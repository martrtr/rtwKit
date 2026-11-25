#!/usr/bin/env python3
"""Build private UI acceptance fixtures from exact local RTWs; no new products."""
from pathlib import Path
from zipfile import ZipFile, ZipInfo, ZIP_DEFLATED
import argparse, hashlib, json

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--artifacts',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
args=parser.parse_args()
artifacts,out=args.artifacts,args.output
out.mkdir(parents=True,exist_ok=True)
if any((out/name).exists() for name in ['interface-fixture.rtw','protocol-default.rtw','protocol-alternative.rtw']):
    raise FileExistsError('refusing to overwrite existing fixtures')
def read(name):
    with ZipFile(artifacts / (name + '.rtw')) as archive:
        return {n: archive.read(n) for n in archive.namelist()}
def digest(data): return 'sha256:' + hashlib.sha256(data).hexdigest()
def write(path, files):
    with ZipFile(path, 'w', compression=ZIP_DEFLATED, compresslevel=9) as z:
        for name, data in sorted(files.items()):
            info = ZipInfo(name, (1980, 1, 1, 0, 0, 0)); info.compress_type = ZIP_DEFLATED; info.external_attr = 0o100644 << 16; info.create_system = 3
            z.writestr(info, data, compress_type=ZIP_DEFLATED, compresslevel=9)
    print(path.name, digest(path.read_bytes()), 'entries', len(files))
settings, web = read('settings-ui'), read('web-ui')
fixture = {k: v for k, v in settings.items() if k.startswith('web/')}
for k, v in web.items():
    if not k.startswith('web/') or k == 'web/index.html': continue
    if k in fixture and fixture[k] != v: raise ValueError('asset collision: ' + k)
    fixture[k] = v
assert fixture['web/index.html'].count(b'<title>Settings</title>') == 1
fixture['web/index.html'] = fixture['web/index.html'].replace(b'<title>Settings</title>', b'<title>Fixture Settings</title>')
needle = b'Welcome to Rintawa.'
assert sum(v.count(needle) for k, v in fixture.items() if k.endswith('.js')) == 1
for k in list(fixture):
    if k.endswith('.js'): fixture[k] = fixture[k].replace(needle, b'Alternative Home Fixture.')
fixture.update({
 'LICENSE': settings['LICENSE'], 'THIRD-PARTY-NOTICES.txt': settings['THIRD-PARTY-NOTICES.txt'],
 'rtw.toml': b'format = 1\ncontent = "rintawa.extension@1"\nentry = "manifest.toml"\n',
 'manifest.toml': b'''id = "rintawa.test.interface"\nname = "Interface Fixture"\nversion = "0.1.0"\nsdk = "^0.0"\n[[components]]\nid = "settings"\nkind = "ui"\ntarget = "rintawa.runtime.web-bundle@2"\nentry = "settings-bundle.toml"\n[components.permissions]\nruntime = ["background-task"]\n[[components]]\nid = "home"\nkind = "ui"\ntarget = "rintawa.runtime.web-bundle@2"\nentry = "home-bundle.toml"\n[components.permissions]\nruntime = ["background-task", "world-session-read"]\n[[components]]\nid = "silent"\nkind = "ui"\ntarget = "rintawa.runtime.web-bundle@2"\nentry = "silent-bundle.toml"\n[components.permissions]\nruntime = ["background-task"]\n''',
 'settings-bundle.toml': b'entry = "web/index.html"\ntitle = "Fixture Settings"\nlaunchable = false\n[[provides]]\nid = "rintawa.ui.settings.renderer"\nversion = 1\n',
 'home-bundle.toml': b'entry = "web/home.html"\ntitle = "Fixture Home"\nlaunchable = false\n[[provides]]\nid = "rintawa.ui.home.renderer"\nversion = 1\n',
 'silent-bundle.toml': b'entry = "web/silent.html"\ntitle = "Silent Fixture"\nlaunchable = false\n',
 'web/silent.html': b'<!doctype html><html><body><main class="silent-fixture"></main></body></html>\n',
 'world-portability.json': b'{"format":1,"properties":[]}\n',
})
write(out/'interface-fixture.rtw', fixture)
dice = (artifacts/'dice.rtw').read_bytes(); iface = (out/'interface-fixture.rtw').read_bytes(); source = read('TextRoleplay')
for alternative in (False, True):
    files = dict(source); template = json.loads(files['template.json']); template['name'] = 'Protocol Alternative' if alternative else 'Protocol Defaults'
    def add(slot, package, data, path):
        sha = digest(data); files[path] = data
        template['artifacts'].append({'digest': sha, 'path': path})
        template['components'].append({'artifact': sha, 'enabled': True, 'packageId': package, 'slot': slot, 'version': '0.1.0'})
    def grant(slot, component, permission): template['composition']['requestedGrants'].append({'slot':slot,'componentId':component,'permission':permission})
    add('dice','rintawa.rtwkit.dice',dice,'extensions/dice.rtw'); grant('dice','dice','background-task')
    if alternative:
        add('fixture','rintawa.test.interface',iface,'extensions/interface-fixture.rtw')
        for component, permissions in [('settings',['background-task']),('home',['background-task','world-session-read']),('silent',['background-task'])]:
            for permission in permissions: grant('fixture',component,permission)
        choices = [p for p in template['composition']['preferredProviders'] if p['contractId']=='rintawa.ui.settings.renderer']
        assert len(choices) == 1; choices[0]['provider'] = {'slot':'fixture','componentId':'settings'}
    files['template.json'] = (json.dumps(template, sort_keys=True, separators=(',',':'))+'\n').encode()
    write(out/('protocol-alternative.rtw' if alternative else 'protocol-default.rtw'),files)
print('input dice',digest(dice)); print('input TextRoleplay',digest((artifacts/'TextRoleplay.rtw').read_bytes()))
