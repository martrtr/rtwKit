"""Exercise a single failed clock read using a temporary WASM runtime build."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib


def main():
    repository = Path(__file__).resolve().parents[1]
    supplied_artifacts = Path(os.environ['RTWKIT_SMOKE_ARTIFACTS']).resolve()
    scratch = Path(tempfile.mkdtemp(prefix='rtwkit-bridge-clock-'))
    print(f'Clock regression build and artifacts at {scratch}', flush=True)
    source = scratch / 'source'
    source.mkdir()
    for name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'LICENSE'):
        shutil.copy2(repository / name, source / name)
    ignored = shutil.ignore_patterns('node_modules', 'build', 'dist', 'target')
    for name in ('packages', 'crates', 'tools', 'wit'):
        shutil.copytree(repository / name, source / name, ignore=ignored)

    runtime = source / 'packages' / 'web-runtime'
    wasm = runtime / 'src' / 'wasm.rs'
    contents = wasm.read_text(encoding='utf-8')
    original = 'let admission = now().and_then(|current| {'
    injected = '''let clock = if request.id == "clock-fault" { Err("unavailable") } else { now() };
                                let admission = clock.and_then(|current| {'''
    if contents.count(original) != 1:
        raise RuntimeError('Expected one authenticated clock admission site')
    wasm.write_text(contents.replace(original, injected), encoding='utf-8')

    artifacts = scratch / 'artifacts'
    version = tomllib.loads((runtime / 'rtwkit.toml').read_text(encoding='utf-8'))['package']['version']
    build_environment = {**os.environ, 'CARGO_TARGET_DIR': str(scratch / 'target')}
    subprocess.run([
        'cargo', 'run', '--locked', '-p', 'rtwkit-registry-tool', '--',
        'build-package', str(source), 'web-runtime', str(artifacts),
        'local/test', f'pkg-web-runtime-v{version}', 'clock-unavailable-fixture',
    ], cwd=source, env=build_environment, check=True)
    for name in ('web-ui.rtw', 'dice.rtw'):
        shutil.copy2(supplied_artifacts / name, artifacts / name)

    subprocess.run([
        sys.executable, str(repository / 'scripts' / 'native-bridge-smoke.py'),
        '--clock-unavailable',
    ], env={**os.environ, 'RTWKIT_SMOKE_ARTIFACTS': str(artifacts)}, check=True)


if __name__ == '__main__':
    main()
