import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class BootstrapTests(unittest.TestCase):
    def run_bootstrap(self, os_name='Linux', arch='x86_64', options=('--dry-run',)):
        with tempfile.TemporaryDirectory(prefix='phantom bootstrap test ') as tmp:
            base = Path(tmp)
            bins = base / 'bin'
            bins.mkdir()
            scripts = {
                'uname': '#!/bin/sh\ncase "$1" in -s) echo "$TEST_OS";; -m) echo "$TEST_ARCH";; esac\n',
                'id': '#!/bin/sh\necho 1000\n',
                'curl': '#!/bin/sh\nwhile [ "$#" -gt 0 ]; do\n if [ "$1" = -o ]; then cp "$TEST_ARCHIVE" "$2"; exit 0; fi\n shift\ndone\nexit 1\n',
            }
            for name, text in scripts.items():
                script = bins / name
                script.write_text(text)
                script.chmod(0o755)
            source = base / 'source/phantom-tkl-driver-main/tools'
            source.mkdir(parents=True)
            (source / 'install.py').write_text(
                'import json,os,sys\nfrom pathlib import Path\n'
                'Path(os.environ["TEST_OUTPUT"]).write_text(json.dumps(sys.argv[1:]))\n')
            archive = base / 'source.tar.gz'
            with tarfile.open(archive, 'w:gz') as tar:
                tar.add(source.parents[0], arcname='phantom-tkl-driver-main')
            output = base / 'output.json'
            scratch = base / 'scratch'
            scratch.mkdir()
            env = dict(os.environ, PATH=str(bins) + os.pathsep + os.environ['PATH'],
                       TEST_OS=os_name, TEST_ARCH=arch, TEST_ARCHIVE=str(archive),
                       TEST_OUTPUT=str(output), TMPDIR=str(scratch))
            # Exercise the same stdin/argument behavior as curl | sh -s -- ...
            result = subprocess.run(['sh', '-s', '--', *options],
                                    input=(ROOT / 'bootstrap.sh').read_text(),
                                    capture_output=True, text=True, env=env, timeout=30)
            forwarded = json.loads(output.read_text()) if output.exists() else None
            leftovers = list(scratch.iterdir())
            return result, forwarded, leftovers

    def test_linux_x86_and_arm_dry_runs(self):
        for arch in ('x86_64', 'aarch64'):
            result, forwarded, leftovers = self.run_bootstrap(arch=arch)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(f'Linux / {arch}', result.stdout)
            self.assertIsNone(forwarded)
            self.assertEqual(leftovers, [])

    def test_macos_arm_alias_dry_run(self):
        result, _, _ = self.run_bootstrap(os_name='Darwin', arch='arm64')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Darwin / aarch64', result.stdout)

    def test_rejects_unsupported_host_before_download(self):
        for os_name, arch in (('FreeBSD', 'x86_64'), ('Linux', 'riscv64')):
            result, forwarded, leftovers = self.run_bootstrap(os_name, arch)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Unsupported', result.stderr)
            self.assertIsNone(forwarded)
            self.assertEqual(leftovers, [])

    def test_pipe_download_forwards_spaced_arguments_and_cleans_source(self):
        args = ('--yes', '--prefix', '/tmp/phantom install with spaces', '--no-device-rules')
        result, forwarded, leftovers = self.run_bootstrap(options=args)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(forwarded, list(args))
        self.assertEqual(leftovers, [])


if __name__ == '__main__':
    unittest.main()
