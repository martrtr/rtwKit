// Real baseline AI settings/WASM, loopback-only model discovery; no credentials.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CDP, eventually, pause, redact } from './browser-driver.mjs';
const core=process.env.RINTAWA_BINARY,artifacts=process.env.RTWKIT_SMOKE_ARTIFACTS;
if(!core||!artifacts)throw Error('Set RINTAWA_BINARY and RTWKIT_SMOKE_ARTIFACTS');
const root=await mkdtemp(join(tmpdir(),'rtwkit-models-')),home=join(root,'home');
let host,chrome,cdp,hostOutput='',chromeOutput='',mode='normal';const checks=[],requests=[],live=new Set();
const server=createServer((request,response)=>{
  assert.equal(request.method,'GET');assert.equal(request.headers.authorization,undefined);requests.push({url:request.url,mode});
  live.add(response);response.on('close',()=>live.delete(response));
  if(mode==='drop'){request.socket.destroy();return;}
  if(mode==='delay'){return;}
  if(mode==='unauthorized'){response.writeHead(401);response.end('private upstream error');return;}
  response.setHeader('content-type','application/json');
  if(mode==='invalid'){response.end('{"models":"private upstream error"}');return;}
  if(mode==='large'){response.end('x'.repeat(128*1024+1));return;}
  if(mode==='empty'){response.end('{"data":[]}');return;}
  response.end(JSON.stringify({data:request.url==='/v2/models'?[{id:'new-destination'}]:[{id:'zeta'},{id:'alpha'},{id:'alpha'}]}));
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const origin=`http://127.0.0.1:${server.address().port}`;
const cli=(...args)=>{const r=spawnSync(core,['--home',home,...args],{encoding:'utf8'});assert.equal(r.status,0,redact(r.stderr));return r.stdout;};
const install=(slug,permissions)=>{const value=cli('install',join(artifacts,slug+'.rtw'));const id=/^instance = (.+)$/m.exec(value)[1];for(const [component,grants]of Object.entries(permissions))for(const grant of grants)cli('grant-runtime',id,component,grant);return id;};
const start=async()=>{let output='';host=spawn(core,['--home',home,'run'],{env:{...process.env,RUST_LOG:'info'},stdio:['ignore','pipe','pipe']});for(const stream of[host.stdout,host.stderr])stream.on('data',b=>{output+=b;hostOutput+=b;});return eventually(()=>{if(host.exitCode!==null)throw Error('Host exited');return /http:\/\/127\.0\.0\.1:\d+\/shell\/[^\s\x1b]+/.exec(output)?.[0];},'Host launch',30000);};
const stop=async()=>{if(host&&host.exitCode===null){host.kill('SIGINT');await Promise.race([new Promise(r=>host.once('exit',r)),pause(10000)]);if(host.exitCode===null)host.kill('SIGKILL');assert.equal(host.exitCode,0);}};
const open=async()=>{await cdp.openSettingsSection('Ai Provider');return eventually(async()=>(await cdp.panelContexts('.ai-settings'))[0],'provider settings');};
const choose=async(selector,value,frame)=>cdp.evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});e.value=${JSON.stringify(value)};e.dispatchEvent(new Event('change',{bubbles:true}));})()`,frame);
try{
 install('web-runtime',{runtime:['background-task','loopback-listen']});install('web-ui',{shell:['background-task','world-session-read','world-session-write','runtime-policy-write'],home:['background-task','world-session-read']});install('settings-ui',{settings:['background-task'],appearance:['background-task']});
 const owner=install('ai-provider',{backend:['background-task'],settings:['background-task']});
 cli('package-install',join(artifacts,'TextRoleplay.rtw'));
 let launch=await start();chrome=spawn('/usr/bin/chromium',['--headless=new','--no-sandbox','--disable-dev-shm-usage','--disable-background-networking','--disable-component-update','--disable-sync','--no-first-run','--no-default-browser-check','--remote-debugging-port=0',`--user-data-dir=${join(root,'chromium')}`,'about:blank'],{stdio:['ignore','ignore','pipe']});chrome.stderr.on('data',b=>chromeOutput+=b);
 const port=await eventually(async()=>{try{return(await readFile(join(root,'chromium/DevToolsActivePort'),'utf8')).split('\n')[0];}catch{return null;}},'debugger');const targets=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();cdp=await CDP.connect(targets.find(t=>t.type==='page').webSocketDebuggerUrl);
 await cdp.call('Target.setAutoAttach',{autoAttach:true,waitForDebuggerOnStart:false,flatten:true});for(const method of['Runtime.enable','Page.enable','Log.enable','Network.enable'])await cdp.call(method);await cdp.call('Emulation.setDeviceMetricsOverride',{width:1280,height:900,deviceScaleFactor:1,mobile:false});
 await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:`{const Native=WebSocket;window.__policyWrites=0;window.WebSocket=class extends Native{constructor(...args){super(...args);this.addEventListener('message',event=>{const v=JSON.parse(event.data);if(this.__dropPolicyId!==undefined&&v.id===this.__dropPolicyId&&v.ok){window.__lostPolicyAck=true;this.__dropPolicyId=undefined;event.stopImmediatePropagation();}});}send(value){const v=JSON.parse(value);if(v.method==='runtime-policy.apply-live-change'){window.__policyWrites++;if(window.__dropNextPolicyAck){window.__dropNextPolicyAck=false;this.__dropPolicyId=v.id;}}super.send(value);}};}`});cdp.rootFrame=(await cdp.call('Page.navigate',{url:launch})).frameId;await cdp.waitShell();
 let frame=await open();
 await cdp.evaluate(`(async()=>{const ws=new WebSocket('ws://'+location.host+'/bridge');let root,n=0;const pending=new Map();await new Promise((resolve,reject)=>{ws.onopen=()=>ws.send(JSON.stringify({v:1,type:'auth',token:sessionStorage.getItem('rintawa.session')}));ws.onerror=reject;ws.onmessage=e=>{const v=JSON.parse(e.data);if(v.type==='ready'){root=v.channel;resolve();}else{const p=pending.get(v.id);if(p){pending.delete(v.id);v.ok?p.resolve(v.value):p.reject(Error(v.error.code));}}};});window.__policyRpc=(method,args)=>new Promise((resolve,reject)=>{const id=String(++n);pending.set(id,{resolve,reject});ws.send(JSON.stringify({v:1,id,channel:root,method,args}));});})()`);
 const inspect=()=>cdp.evaluate(`window.__policyRpc('runtime-policy.live-component',{scopeId:'host',instanceId:${JSON.stringify(owner)},componentId:'backend'})`);
 const before=await inspect();const mounted=await cdp.evaluate('location.href',frame);
 await cdp.click('Add provider',frame);await cdp.click('Ollama',frame);await cdp.setInput('#ai-label','Preserved setup draft',frame);await cdp.setInput('#ai-url',origin+'/v1 ',frame);await cdp.waitText('Permission required',frame);assert.equal(requests.length,0);
 await cdp.click('Allow access…',frame);await cdp.waitText('Allow access');assert.ok((await cdp.body()).includes(origin));
 await cdp.evaluate('window.__dropNextPolicyAck=true');
 await cdp.click('Allow selected');await cdp.waitText('Check current access');assert.equal(await cdp.evaluate('window.__lostPolicyAck'),true);
 const effectiveWhileUnknown=await inspect();assert.ok(effectiveWhileUnknown.granted.includes('http-request'));
 await cdp.click('Check current access');await cdp.waitText('Online',frame);assert.equal(await cdp.evaluate('window.__policyWrites'),1,'reconciliation never repeats the accepted policy change');
 const after=await inspect();assert.deepEqual(after.target,before.target);assert.ok(after.granted.includes('http-request'));assert.deepEqual(after.httpOrigins,[origin]);assert.deepEqual(after.persisted.granted.sort(),after.granted.sort());
 assert.equal(await cdp.evaluate('location.href',frame),mounted);assert.equal(await cdp.evaluate('document.querySelector("#ai-label").value',frame),'Preserved setup draft');assert.equal(await cdp.evaluate('document.querySelector("#ai-url").value',frame),origin+'/v1 ');assert.equal(cdp.worldCalls.filter(c=>c.method==='world-sessions.set-active').length,0);
 checks.push('one grouped live consent grants HTTP/origin without restart, preserves activation/frame/draft, and reconciles a deliberately lost acknowledgement without replay');
 await cdp.setInput('#ai-model','manual-before-approval',frame);await cdp.click('Save provider',frame);await cdp.waitText('Saved.',frame);await eventually(()=>cdp.evaluate('![...document.querySelectorAll("dialog:modal h2")].some(h=>h.textContent==="Allow access")'),'already granted setup needs no extra consent');await cdp.waitText('Online',frame);
 await cdp.click('Edit',frame);await cdp.waitText('Online',frame);assert.deepEqual(await cdp.evaluate('[...document.querySelector("#ai-model-choice").options].slice(1).map(o=>o.value)',frame),['alpha','zeta']);await choose('#ai-model-choice','zeta',frame);assert.equal(await cdp.evaluate('document.querySelector("#ai-model").value',frame),'zeta');await cdp.click('Save provider',frame);await cdp.waitText('Saved.',frame);await cdp.waitText('zeta',frame);assert.ok(requests.every(r=>r.url==='/v1/models'));
 checks.push('automatic GET discovery after approval returns sorted unique model choices; selection saves the model without generation');
 for(const [next,expected]of[['empty','Online'],['invalid','Incompatible response'],['unauthorized','Authentication failed'],['large','Response too large'],['drop','Offline']]){
  mode=next;await cdp.click('Refresh connections',frame);await cdp.waitText(expected,frame);assert.ok(!(await cdp.body(frame)).includes('private upstream error'));
 }
 checks.push('empty inventory stays Online; incompatible/401/oversize/transport failures are distinct and hide upstream bodies');
 mode='normal';await cdp.click('Edit',frame);await cdp.waitText('Online',frame);mode='delay';await cdp.click('Refresh models',frame);await eventually(()=>live.size===1,'held discovery');await cdp.setInput('#ai-url',origin+'/v2',frame);mode='normal';await cdp.waitText('new-destination',frame);assert.deepEqual(await cdp.evaluate('[...document.querySelector("#ai-model-choice").options].slice(1).map(o=>o.value)',frame),['new-destination']);await eventually(()=>live.size===0,'old probe released');
 checks.push('changing destination closes the admitted read and stale results cannot replace the current model list');
 const count=requests.length;await choose('#ai-auth','bearer',frame);await cdp.waitText('Save first',frame);await pause(500);assert.equal(requests.length,count);assert.ok((await cdp.body(frame)).includes('system credentials'));await cdp.click('Cancel',frame);
 checks.push('authentication explains Bearer/system credentials; unsaved changed destination cannot inherit a saved key');
 await cdp.call('Emulation.setDeviceMetricsOverride',{width:640,height:900,deviceScaleFactor:1,mobile:false});await cdp.click('Edit',frame);assert.equal(await cdp.evaluate('(()=>{const r=document.querySelector("dialog:modal").getBoundingClientRect();return r.left>=0&&r.right<=innerWidth;})()',frame),true);await cdp.click('Cancel',frame);
 checks.push('model picker remains within the narrow Settings viewport');
 await cdp.click('Close dialog');
 await cdp.click('New World');
 await cdp.setInput('#world-name','Provider models');
 await eventually(()=>cdp.evaluate('!!document.querySelector(".template-approval input")'),'World permission review');
 await cdp.evaluate('document.querySelector(".template-approval input").click()');
 await cdp.click('Create world');
 await cdp.waitText('Stop World');
 const beforeWorldProbes=requests.length;
 frame=await open();
 await cdp.waitText('Online',frame);
 assert.ok(requests.slice(beforeWorldProbes).some(r=>r.url==='/v1/models'));
 await cdp.click('Add provider',frame);
 await cdp.click('Ollama',frame);
 await cdp.setInput('#ai-label','Second local provider',frame);
 await cdp.setInput('#ai-url',origin+'/v2',frame);
 await cdp.waitText('new-destination',frame);
 assert.equal(await cdp.evaluate('document.querySelector("#ai-model").value',frame),'');
 await choose('#ai-model-choice','new-destination',frame);
 await cdp.click('Save provider',frame);
 await cdp.waitText('Saved.',frame);
 await eventually(()=>cdp.evaluate('document.querySelectorAll(".ai-connection .ai-connectivity.online").length === 2',frame),'primary and additional provider both online');
 const providers=await cdp.evaluate(`[...document.querySelectorAll('.ai-connection')]
   .map(card=>[card.querySelector('h3').textContent,card.querySelector('.ai-selected')?.textContent??null])`,frame);
 assert.deepEqual(providers,[['Preserved setup draft','Primary'],['Second local provider',null]]);
 checks.push('World model discovery works for a keyless draft before model selection and for both primary and additional saved providers');
 assert.deepEqual(cdp.errors,[]);assert.deepEqual(cdp.resourceFailures,[]);console.log(JSON.stringify({root,checks},null,2));
}finally{
 if(cdp){try{await writeFile(join(root,'last-page.png'),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));await writeFile(join(root,'body.txt'),redact(await cdp.body()));}catch{}}
 await writeFile(join(root,'checks.json'),JSON.stringify({checks,errors:cdp?.errors,resourceFailures:cdp?.resourceFailures,requests},null,2));await writeFile(join(root,'host.log'),redact(hostOutput));await writeFile(join(root,'chromium.log'),redact(chromeOutput));
 if(chrome&&chrome.exitCode===null){chrome.kill('SIGTERM');await Promise.race([new Promise(r=>chrome.once('exit',r)),pause(3000)]);if(chrome.exitCode===null)chrome.kill('SIGKILL');}await stop();for(const response of live)response.destroy();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));console.log('Evidence:',root);
}
