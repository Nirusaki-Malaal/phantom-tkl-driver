import importlib.util
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('release', ROOT / 'tools/package_release.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def test_rejects_mismatched_tag(self):
        with self.assertRaises(ValueError):
            release.project_version('v999.0.0')

    def test_extracts_and_smokes_complete_bundle(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            binary = base / 'binary'
            binary.write_text('#!/bin/sh\necho "Phantom Studio"\n')
            archive = release.package(binary, 'x86_64-unknown-linux-gnu', base / 'dist')
            with tarfile.open(archive) as tar:
                files = tar.getnames()
                prefix = f'phantom-studio-{release.project_version()}-linux-x86_64'
                for name in ('phantom-studio', 'install.sh', 'tools/install.py',
                             'docs/DOCUMENTATION.md', 'LICENSE', 'BUILD_INFO.json',
                             'packaging/70-phantom-tkl.rules'):
                    self.assertIn(f'{prefix}/{name}', files)
                self.assertFalse(any('/research/' in name for name in files))
                metadata = json.load(tar.extractfile(f'{prefix}/BUILD_INFO.json'))
                self.assertEqual(metadata['target'], 'x86_64-unknown-linux-gnu')
                self.assertEqual(metadata['version'], release.project_version())

    def test_failed_smoke_does_not_leave_publishable_archive(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            binary = base / 'binary'
            binary.write_text('#!/bin/sh\nexit 1\n')
            with self.assertRaises(Exception):
                release.package(binary, 'x86_64-unknown-linux-gnu', base / 'dist')
            self.assertEqual(list((base / 'dist').glob('*.tar.gz')), [])


if __name__ == '__main__':
    unittest.main()
