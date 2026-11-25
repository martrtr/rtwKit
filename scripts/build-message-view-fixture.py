#!/usr/bin/env python3
"""Package an independent message inspector and a template containing it."""
from pathlib import Path
from zipfile import ZipFile, ZipInfo, ZIP_DEFLATED
import argparse, hashlib, json
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--artifacts', type=Path, required=True)
p.add_argument('--output', type=Path, required=True)
a = p.parse_args(); a.output.mkdir(parents=True, exist_ok=True)
def digest(data): return 'sha256:' + hashlib.sha256(data).hexdigest()
def write(path, files):
    if path.exists(): raise FileExistsError(path)
    with ZipFile(path, 'w', compression=ZIP_DEFLATED) as z:
        for name, data in sorted(files.items()):
            i = ZipInfo(name, (1980,1,1,0,0,0)); i.compress_type = ZIP_DEFLATED; i.external_attr = 0o100644 << 16
            z.writestr(i, data)
with ZipFile(a.artifacts/'web-ui.rtw') as z: files = {n:z.read(n) for n in ('LICENSE','THIRD-PARTY-NOTICES.txt')}
files.update({
 'rtw.toml': b'format = 1\ncontent = "rintawa.extension@1"\nentry = "manifest.toml"\n',
 'manifest.toml': b'id = "rintawa.test.message-view"\nname = "Message view fixture"\nversion = "0.1.0"\nsdk = "^0.0"\n[[components]]\nid = "inspector"\nkind = "ui"\ntarget = "rintawa.runtime.web-bundle@2"\nentry = "bundle.toml"\n[components.permissions]\nruntime = ["background-task"]\n',
 'bundle.toml': b'''entry = "web/index.html"\ntitle = "Message inspector fixture"\nlaunchable = false\n[[contributions]]\npoint = "rintawa/chat/message-views/v1"\npayload = '{"id":"inspector","label":"Message inspector","order":10}'\n''',
 'web/index.html': b'<!doctype html><meta charset="utf-8"><main class="message-view-fixture">Loading</main><script type="module" src="./fixture.js"></script>',
 'web/bridge.js': Path('web/bridge/dist/index.js').read_bytes(),
 'web/fixture.js': r'''import {connectPanel} from './bridge.js';
const root=document.querySelector('main');
try {
 await new Promise(resolve=>setTimeout(resolve,350));
 const client=await connectPanel(); const context=client.initialState;
 const keys=['conversationId','entryId','messageId','expectedRevision','conversationRevision'];
 if(Object.keys(context).sort().join(',')!==keys.slice().sort().join(',') || keys.some(k=>typeof context[k]!=='string')) throw Error('invalid initial state');
 window.__messageFixture={context,client};
 const {conversationId,entryId,messageId,expectedRevision}=context;
 const request=new TextEncoder().encode(JSON.stringify({method:'get-message',conversationId,entryId,messageId,expectedRevision}));
 const replies=await client.query('rintawa/chat/v1',request);
 if(replies.length!==1) throw Error('owner unavailable');
 const result=JSON.parse(new TextDecoder().decode(replies[0])); if(!result.ok) throw Error(result.error.code);
 window.__messageFixture.message=result.value;
 root.textContent=JSON.stringify({context,text:result.value.text});
 window.addEventListener('pagehide',()=>client.dispose(),{once:true});
} catch(error) { root.textContent='Fixture error: '+String(error); window.__messageFixtureError=String(error); }
'''.encode(),
})
fixture=a.output/'message-view-fixture.rtw'; write(fixture,files); data=fixture.read_bytes()
with ZipFile(a.artifacts/'TextRoleplay.rtw') as z: template_files={n:z.read(n) for n in z.namelist()}
t=json.loads(template_files['template.json']); sha=digest(data)
t['name']='Message view acceptance'; t['artifacts'].append({'digest':sha,'path':'extensions/message-view-fixture.rtw'})
t['components'].append({'slot':'message-fixture','artifact':sha,'enabled':True,'packageId':'rintawa.test.message-view','version':'0.1.0'})
t['composition']['requestedGrants'].append({'slot':'message-fixture','componentId':'inspector','permission':'background-task'})
template_files['extensions/message-view-fixture.rtw']=data; template_files['template.json']=(json.dumps(t,sort_keys=True,separators=(',',':'))+'\n').encode()
write(a.output/'MessageViewAcceptance.rtw',template_files)
print(json.dumps({fixture.name:sha,'MessageViewAcceptance.rtw':digest((a.output/'MessageViewAcceptance.rtw').read_bytes())},indent=2))
