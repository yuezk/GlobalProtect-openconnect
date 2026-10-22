import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]


def run_block(path, step=None):
    source = (ROOT / path).read_text()
    if step:
        source = source.split('- name: ' + step + '\n', 1)[1]
    match = re.search(r'(?m)^( +)run: \|\n', source)
    indent = len(match[1]) + 2
    lines = []
    for line in source[match.end():].splitlines():
        if line.strip() and len(line) - len(line.lstrip()) < indent:
            break
        lines.append(line[indent:])
    return '\n'.join(lines)


class PrivateOutputTests(unittest.TestCase):
    def test_private_checkout_supports_branch_tag_and_sha_without_retaining_token(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            origin = root / 'origin'
            subprocess.run(['git', 'init', '-b', 'main', str(origin)], check=True, capture_output=True)
            git_env = dict(os.environ, GIT_AUTHOR_NAME='Test', GIT_AUTHOR_EMAIL='test@example.com',
                           GIT_COMMITTER_NAME='Test', GIT_COMMITTER_EMAIL='test@example.com')
            subprocess.run(['git', '-C', str(origin), 'commit', '--allow-empty', '-m', 'PRIVATE_SUBJECT'],
                           env=git_env, check=True, capture_output=True)
            subprocess.run(['git', '-C', str(origin), 'tag', 'v1.0.0'], check=True)
            sha = subprocess.check_output(['git', '-C', str(origin), 'rev-parse', 'HEAD'], text=True).strip()
            config = root / 'gitconfig'
            config.write_text(f'[url "{origin.as_uri()}"]\n    insteadOf = https://github.com/yuezk/gpgui\n')
            for index, ref in enumerate(('main', 'v1.0.0', sha)):
                destination = root / f'checkout-{index}'
                env = dict(os.environ, GIT_CONFIG_GLOBAL=str(config), PRIVATE_TOKEN='test-token',
                           PRIVATE_REF=ref, PRIVATE_PATH=str(destination))
                result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c',
                                         run_block('.github/actions/checkout-private/action.yaml')],
                                        env=env, text=True, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertNotIn('PRIVATE_SUBJECT', result.stdout + result.stderr)
                self.assertNotIn('test-token', (destination / '.git/config').read_text())
                self.assertNotIn('extraheader', (destination / '.git/config').read_text())
                actual = subprocess.check_output(['git', '-C', str(destination), 'rev-parse', 'HEAD'], text=True)
                self.assertEqual(actual.strip(), sha)

    def test_private_commands_hide_stdout_and_stderr_and_preserve_failure(self):
        cases = [
            ('.github/workflows/build.yaml', 'Build macOS release artifacts'),
            ('.github/workflows/build.yaml', 'Build gpgui frontend'),
            ('.github/workflows/build-gpgui-linux.yaml', 'Build gpgui in Docker'),
            ('.github/workflows/publish-update-feeds.yaml', 'Install Worker tooling'),
            ('.github/workflows/publish-update-feeds.yaml', 'Deploy update feed worker'),
            ('.github/actions/checkout-private/action.yaml', None),
        ]
        for path, step in cases:
            for status in (0, 19):
                with self.subTest(step=step, status=status), tempfile.TemporaryDirectory() as temporary:
                    root = Path(temporary)
                    stub = '#!/bin/sh\necho PRIVATE_STDOUT\necho PRIVATE_STDERR >&2\necho \"MACOS_STAGE: Signing\"\necho \"MACOS_STAGE: Signing PRIVATE_DETAIL\"\nexit "$TEST_STATUS"\n'
                    for command in ('git', 'docker', 'pnpm', 'source/gpgui/scripts/build-macos.sh'):
                        executable = root / command
                        executable.parent.mkdir(parents=True, exist_ok=True)
                        executable.write_text(stub)
                        executable.chmod(0o755)
                    (root / 'gpgui-frontend/gpgui/apps/desktop').mkdir(parents=True)
                    (root / 'gpgui/apps/update-feed-worker/public').mkdir(parents=True)
                    (root / 'updates').mkdir()
                    (root / 'updates/appcast.xml').write_text('fixture')
                    env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}',
                               TEST_STATUS=str(status), CHANNEL='snapshot', GPGUI_BUILDER='fixture',
                               RUNNER_TEMP=str(root), UPDATES_WORKER_NAME='fixture',
                               PRIVATE_TOKEN='test-token', PRIVATE_REF='test-ref', PRIVATE_PATH='private')
                    result = subprocess.run(['bash', '-e', '-c', run_block(path, step)],
                                            cwd=root, env=env, text=True, capture_output=True)
                    self.assertEqual(result.returncode, status, result.stdout + result.stderr)
                    self.assertNotIn('PRIVATE_STD', result.stdout + result.stderr)
                    self.assertNotIn('PRIVATE_DETAIL', result.stdout + result.stderr)
                    if step == 'Build macOS release artifacts':
                        self.assertIn('::notice::MACOS_STAGE: Signing', result.stdout)
                    self.assertNotIn('test-token', result.stdout + result.stderr)
                    self.assertIn('passed' if status == 0 else 'failed', result.stdout)


class PrivateTestStageTests(unittest.TestCase):
    def run_stages(self, mode):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            docker = root / 'docker'
            docker.write_text(textwrap.dedent('''\
                #!/usr/bin/env python3
                import json, os, sys, time
                with open(os.environ['CALLS'], 'a') as output:
                    output.write(json.dumps(sys.argv[1:]) + '\\n')
                print('PRIVATE_STDOUT')
                print('PRIVATE_STDERR', file=sys.stderr)
                if 'cargo' in sys.argv and '--no-run' not in sys.argv:
                    if os.environ['MODE'] == 'hang':
                        time.sleep(60)
                    if os.environ['MODE'] == 'fail':
                        sys.exit(19)
                '''))
            timeout = root / 'timeout'
            timeout.write_text(textwrap.dedent('''\
                #!/usr/bin/env python3
                import subprocess, sys
                assert sys.argv[1].startswith('--kill-after=')
                assert sys.argv[2].endswith(('m', 's'))
                try:
                    result = subprocess.run(sys.argv[3:], timeout=2)
                    sys.exit(result.returncode)
                except subprocess.TimeoutExpired:
                    sys.exit(124)
                '''))
            for executable in (docker, timeout):
                executable.write_text(executable.read_text().replace("#!/usr/bin/env python3", f"#!{sys.executable}"))
                executable.chmod(0o755)
            calls = root / 'calls.jsonl'
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', MODE=mode, CALLS=str(calls))
            result = subprocess.run(['bash', str(ROOT / 'scripts/test-private-desktop.sh'), str(root)],
                                    env=env, text=True, capture_output=True, timeout=10)
            self.assertNotIn('PRIVATE_STD', result.stdout + result.stderr)
            recorded = [json.loads(line) for line in calls.read_text().splitlines()]
            self.assertEqual(recorded[-1][:2], ['rm', '--force'])
            return result

    def test_success_reports_each_stage(self):
        result = self.run_stages('success')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(result.stdout.count(': started'), 7)
        self.assertEqual(result.stdout.count(': passed'), 7)
        self.assertIn('Compile Rust tests: passed', result.stdout)
        self.assertIn('Rust tests: passed', result.stdout)

    def test_failure_names_stage_and_preserves_exit(self):
        result = self.run_stages('fail')
        self.assertEqual(result.returncode, 19)
        self.assertIn('Rust tests: failed', result.stdout)
        self.assertNotRegex(result.stdout, r'(?m)^Rust tests: passed')

    def test_hung_tests_report_timeout_and_cleanup(self):
        result = self.run_stages('hang')
        self.assertEqual(result.returncode, 124)
        self.assertIn('Rust tests: timed out', result.stdout)
        self.assertNotRegex(result.stdout, r'(?m)^Rust tests: passed')


class MacOSArtifactSelectionTests(unittest.TestCase):
    def select(self, jobs, artifacts=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            gh = root / 'gh'
            gh.write_text(textwrap.dedent('''\
                #!/usr/bin/env python3
                import json, os, re, sys
                endpoint = sys.argv[3]
                if endpoint.endswith('/artifacts'):
                    records = json.loads(os.environ['ARTIFACTS'])
                else:
                    attempt = re.search(r'/attempts/([0-9]+)/jobs', endpoint)[1]
                    records = json.loads(os.environ['JOBS']).get(attempt, [])
                for record in records:
                    print(json.dumps(record))
                '''))
            gh.chmod(0o755)
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', JOBS=json.dumps(jobs),
                       ARTIFACTS=json.dumps(artifacts if artifacts is not None else [
                           {'name': 'macos-release-42.1.macos', 'expired': False}]))
            return subprocess.run(['bash', str(ROOT / 'scripts/select-macos-artifact.sh'), 'owner/repo', '42', '3'],
                                  env=env, capture_output=True, text=True)

    def job(self, conclusion='success', status='completed'):
        return {'name': 'build-macos', 'status': status, 'conclusion': conclusion}

    def test_unrelated_reruns_reuse_successful_macos_attempt(self):
        result = self.select({'1': [self.job()]})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'macos-release-42.1.macos')

    def test_newer_macos_execution_wins(self):
        result = self.select({'1': [self.job()], '3': [self.job()]}, [
            {'name': f'macos-release-42.{attempt}.macos', 'expired': False} for attempt in (1, 3)])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'macos-release-42.3.macos')

    def test_never_falls_back_past_failed_cancelled_skipped_or_running_macos(self):
        for conclusion, status in [('failure', 'completed'), ('cancelled', 'completed'),
                                   ('skipped', 'completed'), (None, 'in_progress')]:
            with self.subTest(conclusion=conclusion):
                result = self.select({'1': [self.job()], '2': [self.job(conclusion, status)]})
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, '')

    def test_missing_expired_or_duplicate_artifacts_are_rejected(self):
        valid = {'name': 'macos-release-42.1.macos', 'expired': False}
        for artifacts in ([], [dict(valid, expired=True)], [valid, valid]):
            with self.subTest(artifacts=artifacts):
                self.assertNotEqual(self.select({'1': [self.job()]}, artifacts).returncode, 0)

    def test_missing_macos_job_is_rejected(self):
        self.assertNotEqual(self.select({}).returncode, 0)


if __name__ == '__main__':
    unittest.main()
