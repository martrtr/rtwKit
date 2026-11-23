#!/usr/bin/env python3
"""Install the local browser UI into a new Host home and run it."""
import argparse
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--core', default=os.environ.get('RINTAWA_BINARY'), help='Core CLI binary (or set RINTAWA_BINARY)')
parser.add_argument('--artifacts', type=Path, required=True, help='directory containing web-runtime.rtw, web-ui.rtw, settings-ui.rtw, ai-provider.rtw and TextRoleplay.rtw')
parser.add_argument('--home', type=Path, help='nonexistent or empty directory (default: new temporary directory)')
args = parser.parse_args()
if not args.core:
    parser.error('provide --core or set RINTAWA_BINARY')
core = shutil.which(args.core)
if not core:
    parser.error('Core binary not found or not executable')
core = str(Path(core).resolve())
artifacts = args.artifacts.resolve()
packages = {
    'web-runtime': {'runtime': ['background-task', 'loopback-listen']},
    'web-ui': {'shell': ['background-task', 'world-session-read', 'world-session-write', 'runtime-policy-write'], 'home': ['background-task', 'world-session-read']},
    'settings-ui': {'settings': ['background-task'], 'appearance': ['background-task']},
    'ai-provider': {'backend': ['background-task'], 'settings': ['background-task']},
}
for name in (*packages, 'TextRoleplay'):
    if not (artifacts / f'{name}.rtw').is_file():
        parser.error(f'missing artifact: {artifacts / (name + ".rtw")}')
if args.home is None:
    home = Path(tempfile.mkdtemp(prefix='rintawa-ui-'))
else:
    home = args.home.absolute()
    if home.is_symlink():
        parser.error('--home must not be a symlink')
    try:
        home.mkdir()
    except FileExistsError:
        if not home.is_dir() or any(home.iterdir()):
            parser.error('--home must be a nonexistent or empty directory')
    home = home.resolve()

def cli(*arguments):
    result = subprocess.run([core, '--home', str(home), *map(str, arguments)],
                            text=True, stdout=subprocess.PIPE, check=True)
    return result.stdout

try:
    for name, components in packages.items():
        output = cli('install', artifacts / f'{name}.rtw')
        match = re.search(r'^instance = (.+)$', output, re.MULTILINE)
        if not match:
            raise RuntimeError(f'install did not return an instance ID for {name}')
        for component, grants in components.items():
            for grant in grants:
                cli('grant-runtime', match.group(1), component, grant)
    cli('package-install', artifacts / 'TextRoleplay.rtw')
except (OSError, subprocess.CalledProcessError, ValueError, KeyError, RuntimeError) as error:
    sys.exit(f'Setup failed in {home}: {error}')

# The private bootstrap URL is emitted at info; a dev launch must show it.
os.environ['RUST_LOG'] = 'info'
command = [core, '--home', str(home), 'run']
print(f'Host home: {home}\nRestart with: RUST_LOG=info {shlex.join(command)}', flush=True)
os.execv(core, command)
