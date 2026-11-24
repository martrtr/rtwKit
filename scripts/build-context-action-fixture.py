#!/usr/bin/env python3
"""Test-only ordinary UI providers; no TTS or other product feature."""
from pathlib import Path
from zipfile import ZipFile,ZipInfo,ZIP_DEFLATED
import argparse,hashlib,json
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
def write(path,files):
    if path.exists():raise FileExistsError(path)
    with ZipFile(path,'w',compression=ZIP_DEFLATED) as z:
        for name,data in sorted(files.items()):
            entry=ZipInfo(name,(1980,1,1,0,0,0));entry.compress_type=ZIP_DEFLATED;entry.external_attr=0o100644<<16;z.writestr(entry,data)
def digest(data):return 'sha256:'+hashlib.sha256(data).hexdigest()
with ZipFile(a.artifacts/'web-ui.rtw') as z:base={n:z.read(n) for n in ['LICENSE','THIRD-PARTY-NOTICES.txt']}
base['rtw.toml']=b'format = 1\ncontent = "rintawa.extension@1"\nentry = "manifest.toml"\n'
manifest='id = "rintawa.test.context-actions"\nname = "Context action fixture"\nversion = "0.1.0"\nsdk = "^0.0"\n'
for component,title in [('good','Action fixture'),('rogue','Rogue fixture')]:
    manifest+=f'[[components]]\nid = "{component}"\nkind = "ui"\ntarget = "rintawa.runtime.web-bundle@2"\nentry = "{component}-bundle.toml"\n[components.permissions]\nruntime = ["background-task"]\n'
    base[f'{component}-bundle.toml']=f'entry = "web/{component}.html"\ntitle = "{title}"\nlaunchable = true\n'.encode()
    base[f'web/{component}.html']=f'<!doctype html><meta charset="utf-8"><main class="action-fixture" data-role="{component}">{title}</main><script type="module" src="./fixture.js"></script>'.encode()
base['manifest.toml']=manifest.encode();base['web/bridge.js']=Path('web/bridge/dist/index.js').read_bytes()
base['web/fixture.js']=r'''
import {connectPanel} from './bridge.js';
const role=document.querySelector('main').dataset.role,client=await connectPanel();
const ownPoint=role==='good'?'rintawa/test/inspect/v1':'rintawa/test/unpinned/v1';
const state=window.__fixture={role,effects:[],received:[],point:role==='good'?ownPoint:'rintawa/test/inspect/v1',duplicate:false,hold:false,pending:new Map()};
const encode=v=>new TextEncoder().encode(JSON.stringify(v));
const ask=async(point,call)=>{const replies=await client.query(point,encode(call));if(replies.length!==1)throw Error('unavailable');const value=JSON.parse(new TextDecoder().decode(replies[0]));if(!value.ok)throw Error(value.error.code);return value.value;};
const authorized=async caller=>{const selected=await client.request('runtime-context.selected-providers',{id:'rintawa.chat',version:1});if(selected.length!==1||selected[0].instanceId!==caller.instanceId||caller.componentId!=='ui')return false;const views=await client.views();return views.filter(v=>v.restoreKey.instanceId===caller.instanceId&&v.restoreKey.componentId===caller.componentId&&v.activation===caller.activation).length===1;};
const discovery=await client.serve('rintawa/chat/message-actions/v1',()=>encode({actionPoint:state.point,actions:state.duplicate?[{id:'inspect',label:'Duplicate fixture',order:0},{id:'inspect',label:'Duplicate fixture',order:1}]:[{id:'inspect',label:role==='good'?'Inspect message':'Spoof action',order:role==='good'?10:20}]}));
const invoke=await client.serve(ownPoint,async query=>{
 const call=JSON.parse(new TextDecoder().decode(query.payload));state.received.push(call.method);
 const error=code=>encode({ok:false,error:{code}});
 if(!await authorized(query.caller))return error('access-denied');
 if(call.method==='release'){const operation=state.pending.get(call.operation);if(operation){operation.released=true;if(operation.done)state.pending.delete(call.operation);}return encode({ok:true,value:null});}
 if(call.method==='status'){const operation=state.pending.get(call.operation);if(!operation||operation.released)return error('invalid-operation');return encode({ok:true,value:operation.done?{status:'completed',message:'Inspected saved message'}:{status:'pending',operation:call.operation}});}
 if(call.method!=='invoke'||call.actionId!=='inspect')return error('invalid-request');
 if(state.pending.size>=16)return error('limit-exceeded');
 let message;try{message=await ask('rintawa/chat/v1',{method:'get-message',...call.context});}catch(e){return error(e.message);}
 if(state.hold){
  const bytes=crypto.getRandomValues(new Uint8Array(32));let binary='';for(const byte of bytes)binary+=String.fromCharCode(byte);const token=btoa(binary).replaceAll('+','-').replaceAll('/','_').replaceAll('=','');
  const work={done:false,released:false};state.pending.set(token,work);setTimeout(()=>{state.effects.push({context:call.context,text:message.text,caller:query.caller});work.done=true;if(work.released)state.pending.delete(token);},700);
  return encode({ok:true,value:{status:'pending',operation:token}});
 }
 state.effects.push({context:call.context,text:message.text,caller:query.caller});return encode({ok:true,value:{status:'completed',message:'Inspected saved message'}});
});
window.__fixture.client=client;window.__fixture.ready=true;
window.addEventListener('pagehide',()=>{void discovery.close().catch(()=>{});void invoke.close().catch(()=>{});client.dispose();},{once:true});
'''.encode()
path=a.output/'context-action-fixture.rtw';write(path,base);data=path.read_bytes()
with ZipFile(a.artifacts/'TextRoleplay.rtw') as z:files={n:z.read(n) for n in z.namelist()}
t=json.loads(files['template.json']);t['name']='Refactor acceptance';slot='action-fixture';sha=digest(data)
t['artifacts'].append({'digest':sha,'path':'extensions/context-action-fixture.rtw'});t['components'].append({'slot':slot,'artifact':sha,'enabled':True,'packageId':'rintawa.test.context-actions','version':'0.1.0'})
for component in ['good','rogue']:t['composition']['requestedGrants'].append({'slot':slot,'componentId':component,'permission':'background-task'})
t['composition']['authoritativeProviders'].append({'point':'rintawa/test/inspect/v1','provider':{'slot':slot,'componentId':'good'}})
files['extensions/context-action-fixture.rtw']=data;files['template.json']=(json.dumps(t,sort_keys=True,separators=(',',':'))+'\n').encode();write(a.output/'RefactorAcceptance.rtw',files)
print(json.dumps({path.name:sha,'RefactorAcceptance.rtw':digest((a.output/'RefactorAcceptance.rtw').read_bytes())},indent=2))
