import hashlib
import json
import os
import plistlib
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
    def test_macos_metadata_records_the_bundled_shell_and_rejects_a_different_one(self):
        for gui in ('tauri', 'nsh'):
            with self.subTest(gui=gui), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                tools = root / 'bin'
                tools.mkdir()
                git = tools / 'git'
                git.write_text('#!/bin/sh\nprintf "%040d\\n" 1\n')
                git.chmod(0o755)
                plutil = tools / 'plutil'
                plutil.write_text(f'#!{sys.executable}\nimport plistlib,sys\n'
                                  'with open(sys.argv[-1], "rb") as file:\n'
                                  '    print(plistlib.load(file)[sys.argv[2]])\n')
                plutil.chmod(0o755)
                info = root / 'source/gpgui/target/release/bundle/macos/GP Connect.app/Contents/Info.plist'
                info.parent.mkdir(parents=True)
                info.write_bytes(plistlib.dumps({'GPDesktopShell': gui}))
                artifacts = root / 'release-assets'
                artifacts.mkdir()
                (artifacts / 'fixture.dmg').write_bytes(b'signed fixture')
                env = dict(os.environ, PATH=f'{tools}:{os.environ["PATH"]}', MACOS_GUI='tauri',
                           CHANNEL='stable', RELEASE_TAG='v3.2.0', REQUEST_ID='42.1.macos',
                           BUILD_VERSION='42.1', WORKFLOW_SHA='a' * 40, MACOS_ARTIFACT_VERSION='3.2.0')
                result = subprocess.run(['bash', '-e', '-c', run_block(
                    '.github/workflows/build.yaml', 'Record build identity')],
                    cwd=root, env=env, text=True, capture_output=True)
                metadata = artifacts / 'release-build.json'
                if gui == 'tauri':
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(json.loads(metadata.read_text())['gui'], 'tauri')
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('desktop shell', result.stderr)
                    self.assertFalse(metadata.exists())

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
                               TEST_STATUS=str(status), CHANNEL='snapshot', MACOS_GUI='tauri', GPGUI_BUILDER='fixture',
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


class NativeDependencyCacheTests(unittest.TestCase):
    def run_step(self, step, metadata, status=0):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            helper = root / 'private/scripts/build-macos-native-deps.sh'
            helper.parent.mkdir(parents=True)
            helper.write_text('#!/bin/sh\nprintf "%s\\n" "$TEST_METADATA"\n'
                              'echo PRIVATE_STDERR >&2\nexit "$TEST_STATUS"\n')
            helper.chmod(0o755)
            key = 'a' * 64
            prefix = root / f'tools/gpgui-macos-native-deps/macos-13.0-arm64-{key}'
            output = root / 'output'
            env = dict(os.environ, GPGUI_DIRECTORY=str(root / 'private'),
                       RUNNER_TOOL_CACHE=str(root / 'tools'), GITHUB_OUTPUT=str(output),
                       TEST_METADATA=metadata.format(key=key, prefix=prefix, private=root / 'private'),
                       TEST_STATUS=str(status))
            result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', run_block(
                '.github/actions/macos-native-deps/action.yaml', step)],
                cwd=root, env=env, text=True, capture_output=True)
            return result, output.read_text() if output.exists() else ''

    def test_identity_emits_only_an_opaque_key_and_native_dependency_prefix(self):
        result, output = self.run_step('Identify native dependency cache', 'key={key}\npath={prefix}')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('key=macos-native-deps-' + 'a' * 64, output)
        self.assertNotIn('PRIVATE_', result.stdout + result.stderr)

    def test_private_output_and_checkout_paths_cannot_become_cache_metadata(self):
        for metadata, status in [('PRIVATE_SOURCE_CODE', 0),
                                 ('key={key}\npath={private}', 0),
                                 ('PRIVATE_SOURCE_CODE', 19)]:
            with self.subTest(metadata=metadata, status=status):
                result, output = self.run_step('Identify native dependency cache', metadata, status)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.returncode, status or 1)
                self.assertEqual(output, '')
                self.assertNotIn('PRIVATE_', result.stdout + result.stderr)
                self.assertIn('diagnostics suppressed', result.stdout)

    def test_preparation_hides_private_command_output_and_preserves_failure(self):
        for status in (0, 19):
            with self.subTest(status=status):
                result, output = self.run_step('Build or validate native libraries', 'PRIVATE_SOURCE_CODE', status)
                self.assertEqual(result.returncode, status)
                self.assertEqual(output, '')
                self.assertNotIn('PRIVATE_', result.stdout + result.stderr)
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
                if 'cargo' in sys.argv and '--no-run' in sys.argv:
                    diagnostics = {
                        'compile-type': 'error[E0061]: PRIVATE_SOURCE\\nerror[E0308]: PRIVATE_SOURCE\\nerror[E0061]: PRIVATE_SOURCE',
                        'compile-disk': 'PRIVATE_PATH: No space left on device',
                        'compile-killed': 'PRIVATE_SOURCE (signal: 9, SIGKILL: kill)',
                        'compile-link': 'PRIVATE_PATH: undefined reference to PRIVATE_SYMBOL',
                        'compile-macro': 'error: proc macro panicked\\nPRIVATE_SOURCE',
                    }
                    diagnostic = diagnostics.get(os.environ['MODE'])
                    if diagnostic:
                        print(diagnostic, file=sys.stderr)
                        sys.exit(101)
                if 'cargo' in sys.argv and '--no-run' not in sys.argv:
                    if os.environ['MODE'] == 'hang':
                        time.sleep(60)
                    if os.environ['MODE'] == 'fail':
                        print('test private::test_function ... FAILED')
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
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', MODE=mode, CALLS=str(calls), TMPDIR=str(root))
            result = subprocess.run(['bash', str(ROOT / 'scripts/test-private-desktop.sh'), str(root)],
                                    env=env, text=True, capture_output=True, timeout=10)
            self.assertNotIn('PRIVATE_STD', result.stdout + result.stderr)
            self.assertNotIn('PRIVATE_SOURCE', result.stdout + result.stderr)
            self.assertNotIn('PRIVATE_PATH', result.stdout + result.stderr)
            self.assertNotIn('PRIVATE_SYMBOL', result.stdout + result.stderr)
            self.assertFalse(list(root.glob('tmp.*')), 'Private command output must be deleted')
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
        self.assertIn(hashlib.sha256(b'private::test_function').hexdigest(), result.stdout)
        self.assertNotIn('private::test_function', result.stdout + result.stderr)
        self.assertNotRegex(result.stdout, r'(?m)^Rust tests: passed')

    def test_hung_tests_report_timeout_and_cleanup(self):
        result = self.run_stages('hang')
        self.assertEqual(result.returncode, 124)
        self.assertIn('Rust tests: timed out', result.stdout)
        self.assertNotRegex(result.stdout, r'(?m)^Rust tests: passed')

    def test_compile_failure_reports_only_allowlisted_diagnostics(self):
        cases = {
            'compile-type': 'Rust diagnostic codes: error[E0061] error[E0308]',
            'compile-disk': 'insufficient disk space',
            'compile-killed': 'compiler or linker killed by SIGKILL',
            'compile-link': 'unresolved linker symbols',
            'compile-macro': 'procedural macro failed',
        }
        for mode, expected in cases.items():
            with self.subTest(mode=mode):
                result = self.run_stages(mode)
                self.assertEqual(result.returncode, 101, result.stdout + result.stderr)
                self.assertIn(expected, result.stdout)
                self.assertNotRegex(result.stdout, r'(?m)^Rust tests: started')

    def test_test_diagnostics_hash_names_without_exposing_private_details(self):
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / 'private.log'
            log.write_text('test private::one ... FAILED\nPRIVATE_SOURCE\n'
                           'test private::one ... FAILED\n'
                           'test private::two ... ok\n')
            result = subprocess.run([sys.executable, str(ROOT / 'scripts/private-ci-diagnostics.py'),
                                     'tests', str(log)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            digest = hashlib.sha256(b'private::one').hexdigest()
            self.assertEqual(result.stdout, f'::error::Rust test failure fingerprint: {digest}\n')
            self.assertEqual(result.stderr, '')

    def test_panic_diagnostics_hide_filename_and_payload(self):
        for thread_id in ('', ' (1234)'):
            with self.subTest(thread_id=thread_id), tempfile.TemporaryDirectory() as temporary:
                log = Path(temporary) / 'private.log'
                log.write_text('test private::one ... FAILED\n'
                               f"thread 'private::one'{thread_id} panicked at PRIVATE_FILENAME.rs:321:9:\n"
                               'PRIVATE_PANIC_PAYLOAD\n')
                result = subprocess.run([sys.executable, str(ROOT / 'scripts/private-ci-diagnostics.py'),
                                         'tests', str(log)], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                digest = hashlib.sha256(b'private::one').hexdigest()
                self.assertEqual(result.stdout,
                                 f'::error::Rust test failure fingerprint: {digest}\n'
                                 f'::error::Rust test panic location: {digest} 321:9\n')
                self.assertEqual(result.stderr, '')

    def test_overdue_diagnostics_hash_names_without_exposing_messages(self):
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / 'private.log'
            log.write_text('test private::slow has been running for over 60 seconds\n'
                           'PRIVATE_SOURCE_PATH: PRIVATE_PAYLOAD\n'
                           'test private::slow has been running for over 60 seconds\n')
            result = subprocess.run([sys.executable, str(ROOT / 'scripts/private-ci-diagnostics.py'),
                                     'tests', str(log)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            digest = hashlib.sha256(b'private::slow').hexdigest()
            self.assertEqual(result.stdout, f'::error::Rust overdue test fingerprint: {digest}\n')
            self.assertEqual(result.stderr, '')

    def test_last_running_target_hash_is_stable_without_paths_or_build_hashes(self):
        for source, basename in [('unittests src/lib.rs', 'private_crate'),
                                 ('tests/PRIVATE_HARNESS.rs', 'private_harness')]:
            for prefix, build_hash in [('/PRIVATE_SOURCE_ONE', '0123456789abcdef'),
                                       ('/PRIVATE_SOURCE_TWO', 'fedcba9876543210')]:
                with self.subTest(source=source, prefix=prefix), tempfile.TemporaryDirectory() as temporary:
                    log = Path(temporary) / 'private.log'
                    log.write_text('     Running tests/previous.rs (/PRIVATE_SOURCE/previous-1111111111111111)\n'
                                   f'     Running {source} ({prefix}/target/debug/deps/{basename}-{build_hash})\n'
                                   'PRIVATE_PAYLOAD\n')
                    result = subprocess.run([sys.executable, str(ROOT / 'scripts/private-ci-diagnostics.py'),
                                             'tests', str(log)], capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    identity = f'{source} ({basename})'
                    digest = hashlib.sha256(identity.encode()).hexdigest()
                    self.assertEqual(result.stdout,
                                     f'::error::Rust last running target fingerprint: {digest}\n')
                    self.assertEqual(result.stderr, '')


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
