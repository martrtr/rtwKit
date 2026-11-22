// Real browser -> scoped Chat backend -> System -> COMMIT -> Host restart.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, readFile, writeFile, appendFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CDP, eventually, pause, redact } from './browser-driver.mjs';
const core=process.env.RINTAWA_BINARY, artifacts=process.env.RTWKIT_SMOKE_ARTIFACTS;
if(!core||!artifacts) throw new Error('Set RINTAWA_BINARY and RTWKIT_SMOKE_ARTIFACTS');
const root=await mkdtemp(join(tmpdir(),'rtwkit-chat-'));const home=join(root,'home');const checks=[];
let host,chrome,cdp,launch,ordering;let hostOutput='',chromeOutput='';
function cli(...args){const r=spawnSync(core,['--home',home,...args],{encoding:'utf8'});if(r.status!==0)throw new Error(`CLI ${args[0]} failed: ${r.stderr}`);return r.stdout;}
function install(slug,permissions){const out=cli('install',join(artifacts,`${slug}.rtw`));const id=/^instance = (.+)$/m.exec(out)?.[1];assert.ok(id);for(const [component,grants]of Object.entries(permissions))for(const grant of grants)cli('grant-runtime',id,component,grant);}
install('web-runtime',{runtime:['background-task','loopback-listen']});
install('web-ui',{shell:['background-task','world-session-read','world-session-write']});
install('chat',{backend:['background-task'],ui:['background-task']});
cli('world-default','rintawa.rtwkit.chat');
// Exercise the accepted operator composition pin before creating either World.
const recipe=join(home,'profiles/default-world.toml');
const parsed=spawnSync('python3',['-c',`import sys,tomllib,json\np=tomllib.load(open(sys.argv[1],'rb'))\nprint(json.dumps(next(x for x in p['activations'] if x['subject']=='rintawa.rtwkit.chat')))` ,recipe],{encoding:'utf8'});
if(parsed.status!==0)throw new Error('Cannot read generated default composition');
const activation=JSON.parse(parsed.stdout);
await appendFile(recipe,`\n[[authoritative_providers]]\nscope-id = "default-world"\npoint = "rintawa/chat/v1"\nprovider-instance-id = "${activation['instance-id']}"\nprovider-component-id = "backend"\n`);
async function startHost() {
  let phase = '';
  host = spawn(core, ['--home', home, 'run'], { env: { ...process.env, RUST_LOG: 'info' }, stdio: ['ignore', 'pipe', 'pipe'] });
  const consume = data => { const value = data.toString(); phase += value; hostOutput += value; };
  host.stdout.on('data', consume); host.stderr.on('data', consume);
  return eventually(() => {
    if (host.exitCode !== null) throw new Error('Host exited before launch');
    return /http:\/\/127\.0\.0\.1:\d+\/shell\/[^\s\x1b]+/.exec(phase)?.[0];
  }, 'private Host launch', 20000);
}
async function stopHost() {
  if (!host || host.exitCode !== null) return;
  host.kill('SIGINT');
  await Promise.race([new Promise(resolve => host.once('exit', resolve)), pause(8000)]);
  if (host.exitCode === null) { host.kill('SIGKILL'); await new Promise(resolve => host.once('exit', resolve)); }
}
async function setValue(selector,value,context){await cdp.evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});const proto=e instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;Object.getOwnPropertyDescriptor(proto,'value').set.call(e,${JSON.stringify(value)});e.dispatchEvent(new Event('input',{bubbles:true}));})()`,context);}
async function openChat(){
  await eventually(async()=>{await cdp.click('Refresh');return cdp.evaluate('!!document.querySelector(\'button[title="Open Chat"]\')');},'Chat catalog');
  await cdp.click('Open Chat');
  return eventually(async()=>(await cdp.panelContexts('.chat'))[0],'Chat iframe');
}
try{
  launch=await startHost();
  chrome=spawn(process.env.CHROMIUM_BINARY??'/usr/bin/chromium',['--headless=new','--no-sandbox','--disable-dev-shm-usage','--disable-background-networking','--disable-component-update','--disable-sync','--no-first-run','--no-default-browser-check','--remote-debugging-port=0',`--user-data-dir=${join(root,'chromium')}`,'about:blank'],{stdio:['ignore','ignore','pipe']});
  chrome.stderr.on('data',data=>{chromeOutput+=data.toString();});
  const port=await eventually(async()=>{try{return(await readFile(join(root,'chromium/DevToolsActivePort'),'utf8')).split('\n')[0];}catch{return null;}},'Chromium debugger');
  const targets=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  cdp=await CDP.connect(targets.find(v=>v.type==='page').webSocketDebuggerUrl);
  await cdp.call('Target.setAutoAttach',{autoAttach:true,waitForDebuggerOnStart:false,flatten:true});
  await cdp.call('Emulation.setDeviceMetricsOverride',{width:1280,height:900,deviceScaleFactor:1,mobile:false});
  for(const method of['Runtime.enable','Page.enable','Log.enable'])await cdp.call(method);
  cdp.rootFrame=(await cdp.call('Page.navigate',{url:launch})).frameId;
  await cdp.waitShell();await cdp.waitText('Welcome to Rintawa');await cdp.createEmptyWorld('Acceptance World A');await cdp.click('Start World');await cdp.waitText('Stop World');
  let panel=await openChat();await cdp.openConversationDialog(panel);
  await setValue('#new-title','Acceptance conversation',panel);await cdp.click('Create',panel);
  await cdp.waitText('Message',panel);
  await setValue('#message','Saved through real WASM and SQL COMMIT.',panel);await cdp.click('Send',panel);
  await cdp.waitText('Saved through real WASM and SQL COMMIT.',panel);
  checks.push('real iframe -> domain SDK -> scoped WASM -> authoritative System -> committed message');
  assert.equal(await cdp.evaluate('!!document.querySelector(\'.member-sidebar, .members-sidebar, [aria-label="Members"]\')',panel),false,'no member sidebar');
  await cdp.click('Home'); await cdp.waitText('Welcome to Rintawa');
  const homeShot=await cdp.call('Page.captureScreenshot',{format:'png'}); await writeFile(join(root,'home.png'),Buffer.from(homeShot.data,'base64'));
  assert.equal(await cdp.evaluate('!!document.querySelector(\'button[aria-label="Open world Acceptance World A"] .active-dot\')'),true,'Home preserves active World');
  await cdp.openWorld('Acceptance World A'); await cdp.waitText('Stop World');
  panel=await eventually(async()=>(await cdp.panelContexts('.chat'))[0],'Chat after Home round trip');
  await cdp.selectConversation('Acceptance conversation',panel);await cdp.waitText('Saved through real WASM and SQL COMMIT.',panel);
  checks.push('Home preserves the active World and returning restores its Chat without a member sidebar');
  // Exercise later/earlier IDs through the same public Root bridge and UI principal.
  ordering=await cdp.evaluate(`(async()=>{
    const ws=new WebSocket('ws://'+location.host+'/bridge');let rootChannel;let n=0;const pending=new Map();
    await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('auth timeout')),4000);ws.onopen=()=>ws.send(JSON.stringify({v:1,type:'auth',token:sessionStorage.getItem('rintawa.session')}));ws.onmessage=e=>{const v=JSON.parse(e.data);if(v.type==='ready'){clearTimeout(timer);rootChannel=v.channel;resolve();}else{const p=pending.get(v.id);if(p){pending.delete(v.id);clearTimeout(p.timer);v.ok?p.resolve(v.value):p.reject(new Error(v.error.code));}}};});
    function rpc(channel,method,args={}){const id=String(++n);return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{pending.delete(id);reject(new Error('RPC timeout'));},4000);pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({v:1,id,channel,method,args}));});}
    let mounted;
    try{
      const worlds=await rpc(rootChannel,'world-sessions.list-worlds');const world=worlds.find(w=>w.active);
      const view=(await rpc(rootChannel,'ui.list')).find(v=>v.title==='Chat'&&v.worldId===world.worldId);
      mounted=await rpc(rootChannel,'ui.mount',{id:view.id});const channel=mounted.channel;
      const encode=value=>btoa(unescape(encodeURIComponent(JSON.stringify(value))));
      async function call(method,args={}){
        const handle=await rpc(channel,'transport.begin-query',{point:'rintawa/chat/v1',payload:encode({method,...args})});
        for(let i=0;i<100;i++){const replies=await rpc(channel,'transport.poll-query',{handle});if(replies!==null){if(replies.length!==1)throw new Error('owner count');const result=JSON.parse(decodeURIComponent(escape(atob(replies[0]))));if(!result.ok)throw new Error(result.error.code);return result.value;}await new Promise(r=>setTimeout(r,15));}throw new Error('query timeout');
      }
      async function finish(operation){for(let i=0;i<100;i++){const result=await call('status',{operation:operation.operation});if(result.status!=='pending'){await call('release',{operation:operation.operation});return result;}await new Promise(r=>setTimeout(r,15));}throw new Error('commit timeout');}
      const conv=(await call('list')).items[0].id;
      const old=await call('allocate',{count:2});const fresh=await call('allocate',{count:2});
      const newer=await finish(await call('begin-send',{conversationId:conv,messageId:fresh[0],entryId:fresh[1],text:'Later ID committed first.'}));
      const older=await finish(await call('begin-send',{conversationId:conv,messageId:old[0],entryId:old[1],text:'Must be rejected as stale.'}));
      const page=await call('page',{conversationId:conv});
      return {newer:newer.status,older:older.status,reason:older.reason??'',olderPresent:page.items.some(e=>e.messageId===old[0]),lastMatches:page.conversation.lastEntryId===fresh[1]};
    }finally{if(mounted)await rpc(rootChannel,'ui.unmount',{channel:mounted.channel}).catch(()=>{});ws.close();}
  })()`);
  assert.equal(ordering.newer,'committed');assert.equal(ordering.older,'failed');assert.ok(ordering.reason.includes('stale-entry-id'));assert.equal(ordering.olderPresent,false);assert.equal(ordering.lastMatches,true);
  checks.push('real System rejects late older entry atomically and preserves pagination cursor');
  await cdp.call('Page.reload');await cdp.waitShell();
  panel=await eventually(async()=>(await cdp.panelContexts('.chat'))[0],'Chat after reload');
  await cdp.selectConversation('Acceptance conversation',panel);await cdp.waitText('Saved through real WASM and SQL COMMIT.',panel);await cdp.waitText('Later ID committed first.',panel);
  checks.push('browser reload rereads committed conversation and messages');
  await cdp.createEmptyWorld('Acceptance World B');await eventually(async()=>(await cdp.panelContexts('.chat')).length===0,'World A Chat panel unmounted');await cdp.click('Start World');await cdp.waitText('Stop World');
  const isolated=await openChat();await cdp.waitText('New conversation',isolated);
  await eventually(()=>cdp.evaluate('!!document.querySelector(\'button[aria-label="New conversation"]:not(:disabled)\')',isolated),'World B initial read');
  assert.equal((await cdp.body(isolated)).includes('Acceptance conversation'),false);
  checks.push('second World sharing the UI provider Store cannot see the first World conversation');
  await cdp.openWorld('Acceptance World A'); await cdp.waitText('Stop World');
  await stopHost();await cdp.waitText('Connection closed. Reconnect to continue.');checks.push('lost Root connection exposes explicit reconnect without replay');launch=await startHost();cdp.rootFrame=(await cdp.call('Page.navigate',{url:launch})).frameId;
  await cdp.waitText('Start World');await cdp.click('Start World');await cdp.waitText('Stop World');await cdp.click('Refresh');
  panel=await eventually(async()=>(await cdp.panelContexts('.chat'))[0],'Chat after Host restart');
  await cdp.selectConversation('Acceptance conversation',panel);await cdp.waitText('Saved through real WASM and SQL COMMIT.',panel);await cdp.waitText('Later ID committed first.',panel);
  checks.push('Host restart preserves exact World conversation/message graph');
  assert.deepEqual(cdp.errors,[],'No browser/CSP errors');
  const shot=await cdp.call('Page.captureScreenshot',{format:'png'});await writeFile(join(root,'chat.png'),Buffer.from(shot.data,'base64'));
  await writeFile(join(root,'result.json'),JSON.stringify({status:'passed',checks},null,2));
  console.log(JSON.stringify({status:'passed',checks,artifacts:root}));
}catch(error){
  const panelBodies=cdp?await Promise.all((await cdp.panelContexts('.chat')).map(context=>cdp.body(context).catch(()=>'<gone>'))):[];
  await writeFile(join(root,'failure.json'),JSON.stringify({error:redact(String(error)),checks,ordering,body:await cdp?.body().catch(()=>'<unavailable>'),panelBodies,browserErrors:cdp?.errors??[]},null,2));
  console.error(`Chat smoke failed: ${redact(String(error))}; evidence ${root}`);process.exitCode=1;
}finally{
  cdp?.socket.close();if(chrome&&chrome.exitCode===null)chrome.kill('SIGTERM');await stopHost();
  await writeFile(join(root,'host.log'),redact(hostOutput));await writeFile(join(root,'chromium.log'),redact(chromeOutput));
}
