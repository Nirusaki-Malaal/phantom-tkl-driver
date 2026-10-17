#!/usr/bin/env python3
"""Bundle a native desktop build and smoke-test the extracted executable."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    'x86_64-unknown-linux-gnu': 'linux-x86_64',
    'aarch64-unknown-linux-gnu': 'linux-aarch64',
    'aarch64-apple-darwin': 'macos-aarch64',
    'x86_64-apple-darwin': 'macos-x86_64',
}


def project_version(tag=None):
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    core = tomllib.loads((ROOT / 'crates/phantom-core/Cargo.toml').read_text())['package']['version']
    if version != core:
        raise ValueError('Desktop and core versions must match')
    if tag is not None and tag != f'v{version}':
        raise ValueError(f'Tag {tag!r} does not match v{version}')
    return version


def package(binary, target, output, tag=None):
    version = project_version(tag)
    platform = TARGETS[target]
    binary = Path(binary).resolve()
    if not binary.is_file():
        raise FileNotFoundError(binary)
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    name = f'phantom-studio-{version}-{platform}'
    with tempfile.TemporaryDirectory(prefix='phantom-release-') as tmp:
        stage = Path(tmp) / name
        stage.mkdir()
        shutil.copy2(binary, stage / 'phantom-studio')
        (stage / 'phantom-studio').chmod(0o755)
        for item in ('README.md', 'LICENSE', 'NOTICE', 'install.sh', 'bootstrap.sh', 'docs', 'packaging'):
            source = ROOT / item
            if source.is_dir():
                shutil.copytree(source, stage / item)
            else:
                shutil.copy2(source, stage / item)
        (stage / 'tools').mkdir()
        shutil.copy2(ROOT / 'tools/install.py', stage / 'tools/install.py')
        metadata = {
            'version': version,
            'target': target,
            'source_commit': subprocess.check_output(
                ['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
            'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
            'workflow_run': os.environ.get('GITHUB_RUN_ID'),
        }
        (stage / 'BUILD_INFO.json').write_text(json.dumps(metadata, indent=2) + '\n')
        archive = output / f'{name}.tar.gz'
        try:
            with tarfile.open(archive, 'w:gz') as tar:
                tar.add(stage, arcname=name)
            # Test the bytes extracted from the archive that will be published.
            extracted = Path(tmp) / 'extracted'
            with tarfile.open(archive) as tar:
                tar.extractall(extracted, filter='data')
            result = subprocess.run(
                [extracted / name / 'phantom-studio', '--help'],
                check=True, capture_output=True, text=True, timeout=30)
            if 'Phantom Studio' not in result.stdout:
                raise ValueError('Packaged executable did not identify itself')
        except BaseException:
            archive.unlink(missing_ok=True)
            raise
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', choices=TARGETS)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    parser.add_argument('--tag')
    parser.add_argument('--check-version', action='store_true')
    args = parser.parse_args()
    if args.check_version:
        print(project_version(args.tag))
        return
    if not args.target:
        parser.error('--target is required when packaging')
    binary = args.binary or ROOT / 'target' / args.target / 'release/phantom-studio'
    print(package(binary, args.target, args.output, args.tag))


if __name__ == '__main__':
    main()
