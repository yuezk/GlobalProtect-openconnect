import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


def load(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


versions = load('validate-release-version')
sources = load('record-build-sources')


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args], text=True,
                                   stderr=subprocess.DEVNULL).strip()


def initialize(root):
    root.mkdir(parents=True, exist_ok=True)
    git(root, 'init', '-b', 'main')
    git(root, 'config', 'user.name', 'Release Test')
    git(root, 'config', 'user.email', 'release@example.invalid')


def write_version(root, cargo='3.1.0', flake=None, tag=None):
    flake = flake or cargo
    (root / 'Cargo.toml').write_text(f'[workspace.package]\nversion = "{cargo}"\n')
    (root / 'flake.nix').write_text(f'releaseVersion = "{flake}";\nreleaseTag = "{tag or "v" + flake}";\n')
    (root / 'flake.lock').write_text('{}\n')


def commit(root, title):
    git(root, 'add', '.')
    git(root, 'commit', '-m', title)
    return git(root, 'rev-parse', 'HEAD')


class ReleaseVersionTests(unittest.TestCase):
    def test_main_rejects_old_new_and_snapshot_flakes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for selected, tag in [('3.1.0', 'v3.1.0'), ('3.3.0', 'v3.3.0'), ('3.2.0', 'snapshot')]:
                write_version(root, '3.2.0', selected, tag)
                with self.subTest(selected=selected, tag=tag), self.assertRaises(ValueError):
                    versions.validate(root)
            write_version(root, '3.2.0')
            self.assertEqual(versions.validate(root), '3.2.0')

    def test_snapshot_candidate_is_allowed_outside_main(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            write_version(root, '3.2.0', tag='snapshot')
            self.assertEqual(versions.validate(root, release_tag='snapshot'), '3.2.0')

    def test_rollback_of_both_version_fields_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            initialize(root)
            write_version(root, '3.3.0')
            base = commit(root, 'new main')
            write_version(root, '3.2.0')
            with self.assertRaisesRegex(ValueError, 'older'):
                versions.validate(root, base_ref=base)

    def test_actual_package_version_must_match(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            write_version(root, '3.2.0')
            package = root / 'result'
            (package / 'bin').mkdir(parents=True)
            for name in ('gpclient', 'gpservice', 'gpauth', 'gpgui'):
                tool = package / 'bin' / name
                tool.write_text(f'#!/bin/sh\necho "{name} 3.2.0 (PRIVATE_IDENTITY)"\n')
                tool.chmod(0o755)
            versions.validate(root, package=package)
            (package / 'bin/gpgui').write_text('#!/bin/sh\necho "gpgui 3.1.0 PRIVATE_DETAIL"\n')
            with self.assertRaisesRegex(ValueError, 'gpgui'):
                versions.validate(root, package=package)


class SourceSelectionTests(unittest.TestCase):
    def test_retry_uses_original_sources_after_both_branches_advance(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            paths = [root / 'gp', root / 'gpgui']
            for repo in paths:
                initialize(repo)
                (repo / 'source').write_text('first')
                commit(repo, 'first')
            path, output = root / 'sources.json', root / 'output'
            sources.record(path, *paths, output)
            original = json.loads(path.read_text())
            for repo in paths:
                (repo / 'source').write_text('second')
                commit(repo, 'second')
            sources.record(path, *paths, output)
            self.assertEqual(json.loads(path.read_text()), original)
            self.assertEqual(output.read_text().count(f"gp_sha={original['gp_sha']}"), 2)

    def test_corrupt_retained_sources_do_not_resolve_again(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            path = root / 'sources.json'
            path.write_text('{"gp_sha":"main","gpgui_sha":"main"}')
            with self.assertRaises(ValueError):
                sources.record(path, root, root, root / 'output')


class PromotionTests(unittest.TestCase):
    def test_sources_and_flake_are_committed_together_preserving_main_work(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            repo, origin, artifact = root / 'repo', root / 'origin.git', root / 'artifact'
            initialize(repo)
            write_version(repo)
            old = commit(repo, '3.1')
            git(repo, 'switch', '-c', 'release/next')
            write_version(repo, '3.2.0', '3.1.0')
            (repo / 'feature').write_text('released product feature')
            release = commit(repo, '3.2 source')
            git(repo, 'tag', 'v3.2.0')
            git(repo, 'switch', 'main')
            (repo / 'infrastructure').write_text('preserve newer main work')
            main = commit(repo, 'main infrastructure')
            git(root, 'init', '--bare', str(origin))
            git(repo, 'remote', 'add', 'origin', str(origin))
            git(repo, 'push', 'origin', 'main', 'v3.2.0')
            artifact.mkdir()
            write_version(artifact, '3.2.0')
            (artifact / 'release-candidate.json').write_text(json.dumps({'version': '3.2.0', 'releaseTag': 'v3.2.0'}))
            result = subprocess.run(['bash', str(ROOT / 'scripts/prepare-release-promotion.sh'),
                                     'v3.2.0', str(artifact)], cwd=repo, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(versions.validate(repo), '3.2.0')
            self.assertEqual(git(repo, 'rev-parse', 'main'), main)
            self.assertEqual(git(repo, 'rev-parse', 'v3.2.0'), release)
            self.assertEqual(git(repo, 'show', f'{old}:Cargo.toml'), '[workspace.package]\nversion = "3.1.0"')
            self.assertTrue((repo / 'infrastructure').exists())
            self.assertTrue((repo / 'feature').exists())
            self.assertEqual(git(repo, 'rev-list', '--parents', '-1', 'HEAD').split()[1:], [main, release])
            self.assertEqual(git(repo, 'status', '--porcelain'), '')


class SnapshotBranchTests(unittest.TestCase):
    def run_publish(self, root, expected, version='3.2.0'):
        write_version(root, version, tag='snapshot')
        metadata = {'releaseTag': 'snapshot', 'version': version,
                    'testBranch': 'chore/flake-feature-example', 'expectedTestHead': expected}
        (root / 'release-candidate.json').write_text(json.dumps(metadata))
        env = dict(os.environ, GITHUB_REF_NAME='feature/example')
        # CI checks out the same helper at its selected SHA. Use the real validator here.
        (root / 'scripts').mkdir(exist_ok=True)
        validator = root / 'scripts/validate-release-version.py'
        validator.write_bytes((ROOT / 'scripts/validate-release-version.py').read_bytes())
        return subprocess.run(['bash', str(ROOT / 'scripts/publish-flake-test-branch.sh')],
                              cwd=root, env=env, capture_output=True, text=True)

    def test_branch_publication_and_stale_lease(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            repo, origin = root / 'repo', root / 'origin.git'
            initialize(repo)
            write_version(repo, '3.2.0')
            base = commit(repo, 'source')
            git(root, 'init', '--bare', str(origin))
            git(repo, 'remote', 'add', 'origin', str(origin))
            first = self.run_publish(repo, '')
            self.assertEqual(first.returncode, 0, first.stdout + first.stderr)
            head = git(repo, 'rev-parse', 'HEAD')
            self.assertEqual(git(origin, 'rev-parse', 'refs/heads/chore/flake-feature-example'), head)
            git(repo, 'switch', '--detach', base)
            git(repo, 'branch', '-D', 'chore/flake-feature-example')
            # Another run already created the branch after this run captured an absent head.
            stale = self.run_publish(repo, '', '3.2.1')
            self.assertNotEqual(stale.returncode, 0)
            self.assertIn('stale info', stale.stderr)
            self.assertEqual(git(origin, 'rev-parse', 'refs/heads/chore/flake-feature-example'), head)
            git(repo, 'switch', '--detach', base)
            git(repo, 'branch', '-D', 'chore/flake-feature-example')
            fresh = self.run_publish(repo, head, '3.2.1')
            self.assertEqual(fresh.returncode, 0, fresh.stdout + fresh.stderr)
            self.assertNotEqual(git(origin, 'rev-parse', 'refs/heads/chore/flake-feature-example'), head)
            self.assertEqual(git(origin, 'branch', '--list', 'main'), '')


class DockerMatrixTests(unittest.TestCase):
    def test_workflow_matrix_follows_all_architecture_choices(self):
        source = (ROOT / '.github/workflows/release.yaml').read_text()
        block = source.split('          const inputs = ${{ toJson(inputs) }}\n', 1)[1].split('\n  build:', 1)[0]
        # Execute the actual github-script block, including both returned matrices.
        lines = []
        for line in block.splitlines():
            if line.strip() and not line.startswith('          '):
                break
            lines.append(line[10:])
        js = '\n'.join(lines)
        for arch, expected in [('all', ['linux/amd64', 'linux/arm64']),
                               ('x86_64', ['linux/amd64']), ('arm64', ['linux/arm64'])]:
            harness = 'const core={setOutput:(k,v)=>console.log(v)}; const inputs=' + json.dumps({'arch': arch}) + ';\n'
            result = subprocess.check_output(['node', '-e', harness + '(function(){\n' + js + '\n})();'], text=True)
            matrix = json.loads(result.splitlines()[0])
            self.assertEqual([row['platform'] for row in matrix['include']], expected)


if __name__ == '__main__':
    unittest.main()
