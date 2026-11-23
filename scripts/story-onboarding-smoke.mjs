// Actual installed-template -> Character welcome -> linked Chat -> restart flow.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, readFile, writeFile, mkdir, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CDP, eventually, pause, redact } from './browser-driver.mjs';
const core = process.env.RINTAWA_BINARY, artifacts = process.env.RTWKIT_SMOKE_ARTIFACTS;
if (!core || !artifacts) throw new Error('Set RINTAWA_BINARY and RTWKIT_SMOKE_ARTIFACTS');
const root = await mkdtemp(join(tmpdir(), 'rtwkit-story-entry-')), home = join(root, 'home');
let activeHome = home;
let aiFixture; const retiredViews = new Set();
let host, chrome, cdp, hostOutput = '', chromeOutput = ''; const checks = [];
function cli(...args) { const result = spawnSync(core, ['--home', activeHome, ...args], { encoding: 'utf8' }); assert.equal(result.status, 0, redact(result.stderr)); return result.stdout; }
function install(slug, component, grants) { const result = cli('install', join(artifacts, `${slug}.rtw`)); const id = /^instance = (.+)$/m.exec(result)?.[1]; assert.ok(id); for (const grant of grants) cli('grant-runtime', id, component, grant); return id; }
async function startHost() {
  let output = '';
  host = spawn(core, ['--home', activeHome, 'run'], { env: { ...process.env, RUST_LOG: 'info' }, stdio: ['ignore', 'pipe', 'pipe'] });
  const consume = bytes => { output += bytes.toString(); hostOutput += bytes.toString(); }; host.stdout.on('data', consume); host.stderr.on('data', consume);
  return eventually(() => { if (host.exitCode !== null) throw new Error('Host exited before launch'); return /http:\/\/127\.0\.0\.1:\d+\/shell\/[^\s\x1b]+/.exec(output)?.[0]; }, 'Host launch', 30000);
}
async function stopHost() { if (!host || host.exitCode !== null) return; host.kill('SIGINT'); await Promise.race([new Promise(resolve => host.once('exit', resolve)), pause(10000)]); if (host.exitCode === null) { host.kill('SIGKILL'); await new Promise(resolve => host.once('exit', resolve)); } assert.equal(host.exitCode, 0, 'clean Host shutdown'); }
async function panel(selector) { return eventually(async () => (await cdp.panelContexts(selector))[0], `${selector} panel`, 30000); }
async function create(name) {
  await cdp.click('New World');
  await eventually(() => cdp.evaluate('document.querySelector("#world-template")?.selectedOptions[0]?.textContent === "Text Roleplay"'), 'installed Text Roleplay option');
  await cdp.setInput('#world-name', name);
  assert.equal(await cdp.evaluate('document.querySelector("dialog button[type=submit]").disabled'), true, 'permission approval is explicit');
  await eventually(() => cdp.evaluate('(()=>{const box=document.querySelector(".template-approval input");if(!box)return false;box.click();return true})()'), 'template permission review');
  await cdp.click('Create world'); await cdp.waitText('Stop World'); return panel('.story-welcome');
}
async function upload(path, context) {
  await cdp.call('DOM.enable', {}, context.sessionId);
  const value = await cdp.call('Runtime.evaluate', { expression: 'document.querySelector("input[type=file]")', contextId: context.id }, context.sessionId);
  const { node } = await cdp.call('DOM.describeNode', { objectId: value.result.objectId }, context.sessionId);
  await cdp.call('DOM.setFileInputFiles', { backendNodeId: node.backendNodeId, files: [path] }, context.sessionId);
  await cdp.call('Runtime.releaseObject', { objectId: value.result.objectId }, context.sessionId);
}
async function admin() {
  await cdp.evaluate(`(async()=>{
    const ws = new WebSocket('ws://'+location.host+'/bridge');const pending=new Map();let sequence=0,root;
    await new Promise((resolve,reject)=>{ws.onopen=()=>ws.send(JSON.stringify({v:1,type:'auth',token:sessionStorage.getItem('rintawa.session')}));ws.onerror=()=>reject(Error('auth failed'));ws.onmessage=e=>{const value=JSON.parse(e.data);if(value.type==='ready'){root=value.channel;resolve();return;}const p=pending.get(value.id);if(p){pending.delete(value.id);clearTimeout(p.timer);value.ok?p.resolve(value.value):p.reject(Error(value.error.code));}};});
    const rpc=(channel,method,args={})=>{const id=String(++sequence);return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{pending.delete(id);reject(Error('timeout'));},5000);pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({v:1,id,channel,method,args}));});};
    window.__storyAdmin={rpc:(method,args)=>rpc(root,method,args),channelCall:(channel,method,args)=>rpc(channel,method,args),close:()=>ws.close(),query:async(worldId,component,point,call)=>{
      const views=await rpc(root,'ui.list');const matches=views.filter(v=>v.worldId===worldId&&v.title===component);if(matches.length!==1)throw Error('ambiguous test view');const binding=await rpc(root,'ui.mount',{id:matches[0].id});
      try {const handle=await rpc(binding.channel,'transport.begin-query',{point,payload:btoa(String.fromCharCode(...new TextEncoder().encode(JSON.stringify(call))))});for(let i=0;i<300;i++){const replies=await rpc(binding.channel,'transport.poll-query',{handle});if(replies!==null){if(replies.length===0)throw Error('unavailable');if(replies.length!==1)throw Error('ambiguous test owner');return JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(replies[0]),c=>c.charCodeAt(0))));}await new Promise(r=>setTimeout(r,20));}throw Error('query timeout');}
      finally {await rpc(root,'ui.unmount',{channel:binding.channel});}
    }};
  })()`);
}
const query = (world, component, point, call) => cdp.evaluate(`window.__storyAdmin.query(${[world, component, point, call].map(v => JSON.stringify(v)).join(',')})`);
const worlds = () => cdp.evaluate("window.__storyAdmin.rpc('world-sessions.list-worlds')");
const card = (version, name) => ({ spec: `chara_card_v${version}`, spec_version: `${version}.0`, data: { name, description: 'Local synthetic onboarding fixture', personality: '', scenario: '', first_mes: '', mes_example: '', creator_notes: '', system_prompt: '', post_history_instructions: '', alternate_greetings: [], tags: [], creator: 'test', character_version: '1', extensions: {} } });
try {
  install('web-runtime', 'runtime', ['background-task', 'loopback-listen']);
  install('web-ui', 'shell', ['background-task', 'world-session-read', 'world-session-write', 'runtime-policy-write']);
  const shellProfile=await readFile(join(home,'profiles/baseline.toml'),'utf8');const shellId=/instance-id = "([^"]+)"/.exec(shellProfile.split('[[activations]]').find(value=>value.includes('subject = "rintawa.rtwkit.web-ui"')))[1];
  for(const permission of ['background-task','world-session-read'])cli('grant-runtime',shellId,'home',permission);
  install('settings-ui','settings',['background-task']);const settingsProfile=await readFile(join(home,'profiles/baseline.toml'),'utf8');const settingsId=/instance-id = "([^"]+)"/.exec(settingsProfile.split('[[activations]]').find(value=>value.includes('subject = "rintawa.rtwkit.settings-ui"')))[1];cli('grant-runtime',settingsId,'appearance','background-task');
  if (process.env.RTWKIT_AI_SMOKE === '1') { const value=cli('install',join(artifacts,'ai-provider.rtw')); const id=/^instance = (.+)$/m.exec(value)[1]; cli('grant-runtime',id,'backend','background-task');cli('grant-runtime',id,'settings','background-task'); }
  cli('package-install', join(artifacts, 'TextRoleplay.rtw'));
  assert.match(cli('template-list'), /Text Roleplay/); assert.doesNotMatch(cli('list'), /rtwkit.character|rtwkit.chat/);
  const firstCard = join(root, process.env.RTWKIT_AI_SMOKE === '1' ? 'alice-v3.png' : 'alice-v3.json'), secondCard = join(root, 'bob-v2.json');
  const firstDocument=card(3,'Story Alice');
  if(process.env.RTWKIT_AI_SMOKE==='1') { firstDocument.data.first_mes='Hello {{user}}, I am {{char}}.'; firstDocument.data.system_prompt='Play {{char}} opposite {{user}}.';firstDocument.data.post_history_instructions='Keep the garden scene.'; }
  if(process.env.RTWKIT_AI_SMOKE==='1') firstDocument.data.description='Large character context: '+ 'descriptive '.repeat(1000);
  await writeFile(firstCard,process.env.RTWKIT_AI_SMOKE === '1' ? (await import('./ai-story-checks.mjs')).characterPng(firstDocument) : JSON.stringify(card(3,'Story Alice'))); await writeFile(secondCard, JSON.stringify(card(2, 'Story Bob')));
  const launch = await startHost();
  chrome = spawn(process.env.CHROMIUM_BINARY ?? '/usr/bin/chromium', ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--disable-background-networking', '--disable-component-update', '--disable-sync', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${join(root, 'chromium')}`, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
  chrome.stderr.on('data', bytes => { chromeOutput += bytes.toString(); });
  const port = await eventually(async () => { try { return (await readFile(join(root, 'chromium/DevToolsActivePort'), 'utf8')).split('\n')[0]; } catch { return null; } }, 'Chromium debugger');
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json(); cdp = await CDP.connect(targets.find(target => target.type === 'page').webSocketDebuggerUrl);
  await cdp.call('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true });
  await cdp.call('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
  for (const method of ['Runtime.enable', 'Page.enable', 'Log.enable', 'Network.enable']) await cdp.call(method);
  cdp.rootFrame = (await cdp.call('Page.navigate', { url: launch })).frameId; await cdp.waitShell();
  let welcome = await create('Story A'); await cdp.waitText('Import a character', welcome); checks.push('installed template chooser -> auto-start -> exact Character welcome');
  // Refuse precisely the first Chat begin before Native dispatch; import must survive.
  await cdp.evaluate(`(()=>{const send=WebSocket.prototype.send;window.__failNextChat=true;WebSocket.prototype.send=function(data){let value;try{value=JSON.parse(data)}catch{};if(window.__failNextChat&&value?.method==='transport.begin-query'&&value.args.point==='rintawa/chat/v1'){const call=JSON.parse(atob(value.args.payload));if(call.method==='begin-create'){window.__failNextChat=false;queueMicrotask(()=>this.dispatchEvent(new MessageEvent('message',{data:JSON.stringify({v:1,id:value.id,channel:value.channel,ok:false,error:{code:'unavailable',outcome:'not-started'}})})));return;}}return send.call(this,data);};})()`);
  await upload(firstCard, welcome); await cdp.waitText('Continue to chat', welcome);
  await eventually(() => cdp.evaluate('!document.querySelector(".story-welcome button")?.disabled', welcome), 'continuation enabled after refused Chat creation');
  await admin(); const first = (await worlds()).find(w => w.title === 'Story A'); assert.ok(first.active);
  const characters = await query(first.worldId, 'Characters', 'rintawa/character/v1', { method: 'list' }); assert.equal(characters.value.items.length, 1);
  const chatsBefore = await query(first.worldId, 'Story', 'rintawa/chat/v1', { method: 'list' }); assert.equal(chatsBefore.value.items.length, 0);
  checks.push('Chat creation refusal preserves committed Character and offers explicit Continue');
  await cdp.click('Continue to chat', welcome); let chat = await panel('.chat'); await cdp.waitText('Story Alice', chat);
  assert.equal(await cdp.evaluate('document.querySelector(".chat").classList.contains("navigation-closed")', chat), true);
  await eventually(async () => JSON.stringify(await cdp.evaluate('[...document.querySelectorAll(".world-apps button")].map(button=>button.textContent.trim())')) === JSON.stringify(['Chat']), 'only Chat in primary navigation');
  for (const width of [1280, 640]) {
    await cdp.call('Emulation.setDeviceMetricsOverride', { width, height: 900, deviceScaleFactor: 1, mobile: false });
    for (const expanded of [true, false, true, false]) {
      await cdp.click('Toggle conversations', chat);
      assert.equal(await cdp.evaluate('document.querySelector(".navigation-toggle").getAttribute("aria-expanded")', chat), String(expanded));
      assert.equal(await cdp.evaluate('getComputedStyle(document.querySelector(".conversation-sidebar")).display !== "none"', chat), expanded);
      assert.equal(await cdp.evaluate('document.querySelector(".thread-refresh").nextElementSibling === document.querySelector(".navigation-toggle")', chat), true);
    }
  }
  await cdp.call('Emulation.setDeviceMetricsOverride', { width:1280,height:900,deviceScaleFactor:1,mobile:false });
  checks.push('only Chat shortcut; drawer opens and closes beside Refresh at desktop and narrow widths');
  await cdp.setInput('#message', 'Saved from the template workflow', chat); await cdp.click('Send', chat); await cdp.waitText('Saved from the template workflow', chat);
  await cdp.click('Workspace controls');
  assert.deepEqual(await cdp.evaluate('[...document.querySelectorAll("#workspace-menu button")].map(button=>button.textContent.trim()).filter(text=>text.startsWith("Split ")).sort()'), ['Split Characters', 'Split Chat']);
  await cdp.click('Split Chat');
  await eventually(async () => (await cdp.panelContexts('.chat')).length === 2, 'Split Chat opens two Chat panels directly');
  for (const context of await cdp.panelContexts('.chat')) await cdp.waitText('Saved from the template workflow', context);
  assert.equal((await cdp.panelContexts('.story-welcome')).length, 0);
  assert.deepEqual(await cdp.evaluate('[...document.querySelectorAll(".dv-tab")].map(tab=>tab.textContent.trim())'), ['Chat', 'Chat']);
  await cdp.evaluate('document.querySelectorAll(".dv-default-tab-action")[1].click()');
  await eventually(async () => (await cdp.panelContexts('.chat')).length === 1, 'closing split leaves original Chat');
  chat = await panel('.chat');
  checks.push('workspace excludes Split Story, retains Chat/Characters; direct Chat split preserves both transcripts without mounting Story');
  const chats = await query(first.worldId, 'Story', 'rintawa/chat/v1', { method: 'list' }); assert.equal(chats.value.items.length, 1); assert.equal(chats.value.items[0].conversation.characterId, characters.value.items[0].id);
  assert.equal((await query(first.worldId, 'Chat', 'rintawa/character/v1', { method: 'allocate', count: 1 })).error.code, 'access-denied');
  const ids = (await query(first.worldId, 'Story', 'rintawa/chat/v1', { method: 'allocate', count: 2 })).value;
  assert.equal((await query(first.worldId, 'Story', 'rintawa/chat/v1', { method: 'begin-send', conversationId: chats.value.items[0].id, messageId: ids[0], entryId: ids[1], text: 'must be denied' })).error.code, 'access-denied');
  checks.push('one linked Chat, saved message, collapsed drawer, narrow workflow authority');
  welcome = await create('Story B'); await cdp.waitText('Import a character', welcome); await upload(secondCard, welcome); chat = await panel('.chat'); await cdp.waitText('Story Bob', chat);
  const second = (await worlds()).find(w => w.title === 'Story B'); assert.notEqual(second.worldId, first.worldId);
  const cardsB = await query(second.worldId, 'Characters', 'rintawa/character/v1', { method: 'list' }); assert.equal(cardsB.value.items.length, 1); assert.equal(cardsB.value.items[0].card.name, 'Story Bob');
  const chatsB = await query(second.worldId, 'Story', 'rintawa/chat/v1', { method: 'list' }); assert.equal(chatsB.value.items.length, 1);
  assert.equal((await query(second.worldId, 'Story', 'rintawa/chat/v1', { method: 'recent', conversationId: chats.value.items[0].id })).error.code, 'not-found');
  checks.push('Tavern V2 and V3 in two independent concurrently running template Worlds');
  if (process.env.RTWKIT_AI_SMOKE === '1') {
    const oversize=card(3,'Large greeting');oversize.data.first_mes='x'.repeat(16385);const path=join(root,'oversize-greeting.json');await writeFile(path,JSON.stringify(oversize));
    welcome=await create('Greeting bounds');await cdp.waitText('Import a character',welcome);await upload(path,welcome);await cdp.waitText('Start without greeting',welcome);
    const bounded=(await worlds()).find(w=>w.title==='Greeting bounds');
    assert.equal((await query(bounded.worldId,'Characters','rintawa/character/v1',{method:'list'})).value.items.length,1);
    assert.equal((await query(bounded.worldId,'Story','rintawa/chat/v1',{method:'list'})).value.items.length,0,'oversize greeting never creates a partial Chat');
    await cdp.click('Start without greeting',welcome);chat=await panel('.chat');await cdp.waitText('Large greeting',chat);
    const created=(await query(bounded.worldId,'Chat','rintawa/chat/v1',{method:'list'})).value.items[0];
    assert.equal((await query(bounded.worldId,'Chat','rintawa/chat/v1',{method:'recent',conversationId:created.id})).value.items.length,0);
    for(const view of await cdp.evaluate("window.__storyAdmin.rpc('ui.list')"))if(view.worldId===bounded.worldId)retiredViews.add(view.id);
    await cdp.click('Workspace controls');await cdp.click('Delete World…');await cdp.click('Delete World');await eventually(async()=>!(await worlds()).some(w=>w.worldId===bounded.worldId),'bounded greeting test World deleted');
    checks.push('oversized greeting keeps Character, creates no partial Chat, and can be explicitly skipped');
  }

  await cdp.openWorld('Story A'); chat = await panel('.chat'); await cdp.waitText('Saved from the template workflow', chat);
  await eventually(() => cdp.evaluate('!!document.querySelector("#message") && !document.querySelector("#message").disabled && !document.body.innerText.includes("Connecting…")',chat),'resumed composer ready');
  await eventually(() => cdp.evaluate('!document.querySelector(".shell-error")'),'no stale shell error');
  const destinationB = (await query(second.worldId, 'Story', 'rintawa/chat/v1', { method: 'describe' })).value;
  await cdp.evaluate(`(()=>{const send=MessagePort.prototype.postMessage;MessagePort.prototype.postMessage=function(value,...rest){if(value?.method)window.__storyPort=this;return send.call(this,value,...rest);};})()`,chat);
  await cdp.click('Refresh',chat);
  await eventually(()=>cdp.evaluate('!!window.__storyPort',chat),'capture actual panel port for negative navigation');
  const navigation = await cdp.evaluate(`new Promise((resolve,reject)=>{const port=window.__storyPort;const timer=setTimeout(()=>reject(Error('navigation timeout')),5000);const id='foreign-navigation';const receive=e=>{if(e.data?.id===id){clearTimeout(timer);port.removeEventListener('message',receive);resolve(e.data);}};port.addEventListener('message',receive);port.postMessage({v:1,id,method:'ui.navigate',args:{instanceId:${JSON.stringify(destinationB.instanceId)},componentId:'ui',state:{}}});})`,chat);
  assert.equal(navigation.ok,false);assert.equal(navigation.error.code,'unavailable');
  assert.ok((await cdp.body()).includes('Story A'));
  checks.push('actual panel MessagePort refuses navigation to another World');
  await eventually(() => cdp.evaluate('!document.body.innerText.includes("Working…") && document.body.innerText.includes("Saved from the template workflow")',chat),'read completes after refused navigation');
  const screenshot = await cdp.call('Page.captureScreenshot', { format: 'png' }); await writeFile(join(root, 'story.png'), Buffer.from(screenshot.data, 'base64'));
  await cdp.evaluate('window.__storyAdmin.close()'); await stopHost();
  const restarted = await startHost(); cdp.rootFrame = (await cdp.call('Page.navigate', { url: restarted })).frameId; await cdp.waitShell(); await cdp.openWorld('Story A'); await cdp.click('Start World'); await cdp.waitText('Stop World');
  chat = await panel('.chat'); await cdp.waitText('Saved from the template workflow', chat); await admin();
  assert.equal((await query(first.worldId, 'Characters', 'rintawa/character/v1', { method: 'list' })).value.items.length, 1);
  assert.equal((await query(first.worldId, 'Story', 'rintawa/chat/v1', { method: 'list' })).value.items.length, 1);
  checks.push('restart restores current conversation and saved message without duplicate Character/Chat');
  // Exercise native top-layer menu over an empty World, where the former overlay hid it.
  const empty = await cdp.evaluate("window.__storyAdmin.rpc('world-sessions.create')");
  await cdp.evaluate(`window.__storyAdmin.rpc('world-sessions.set-metadata',{worldId:${JSON.stringify(empty.worldId)},title:'Empty menu world',description:null,cover:null})`);
  await cdp.click('Home'); await cdp.click('Refresh worlds'); await cdp.openWorld('Empty menu world');
  for (let i=0;i<3;i++) {
    await cdp.click('Workspace controls');
    await eventually(()=>cdp.evaluate('document.querySelector("#workspace-menu").matches(":popover-open")'),'menu opens over empty World');
    assert.equal(await cdp.evaluate('(()=>{const b=[...document.querySelectorAll("#workspace-menu button")].find(b=>b.textContent==="Export World…");const r=b.getBoundingClientRect();return document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)===b;})()'),true);
    if(i===2){await cdp.call('Input.dispatchMouseEvent',{type:'mousePressed',x:140,y:24,button:'left',clickCount:1});await cdp.call('Input.dispatchMouseEvent',{type:'mouseReleased',x:140,y:24,button:'left',clickCount:1});}
    else {await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27,nativeVirtualKeyCode:27});await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27,nativeVirtualKeyCode:27});}
    await eventually(()=>cdp.evaluate('!document.querySelector("#workspace-menu").matches(":popover-open")'),'menu Escape closes');
  }
  await cdp.click('Workspace controls'); await cdp.click('Delete World…'); await cdp.click('Cancel');
  assert.ok((await worlds()).some(w=>w.worldId===empty.worldId),'cancel deletion preserves World');
  await cdp.click('Workspace controls'); await cdp.click('Delete World…'); await cdp.click('Delete World');
  await eventually(async()=>!(await worlds()).some(w=>w.worldId===empty.worldId),'empty World deleted');
  checks.push('workspace popover works repeatedly over empty World; Escape and delete confirmation/cancellation work');
  // Keep B running while exporting A and deleting its independent imported clone.
  // Deterministically reproduce query declarations lagging behind panel mount.
  await cdp.evaluate(`(()=>{const send=WebSocket.prototype.send;window.__startupReadFailures=2;WebSocket.prototype.send=function(data){let value;try{value=JSON.parse(data)}catch{};if(window.__startupReadFailures>0&&value?.method==='transport.begin-query'&&value.args.point==='rintawa/chat/v1'&&JSON.parse(atob(value.args.payload)).method==='list'){window.__startupReadFailures--;queueMicrotask(()=>this.dispatchEvent(new MessageEvent('message',{data:JSON.stringify({v:1,id:value.id,channel:value.channel,ok:false,error:{code:'unavailable',outcome:'not-started'}})})));return;}return send.call(this,data);};})()`);
  await cdp.openWorld('Story B'); await cdp.click('Start World'); await cdp.waitText('Stop World');
  chat=await panel('.chat');await cdp.waitText('Story Bob',chat);
  assert.equal(await cdp.evaluate('window.__startupReadFailures'),0,'startup list reads recover from transient unavailability');
  checks.push('Chat startup restores its conversation after transient backend unavailability without replaying mutations');await cdp.setInput('#message','World B remains usable',chat);await cdp.click('Send',chat);await cdp.waitText('World B remains usable',chat);
  await cdp.openWorld('Story A');await cdp.waitText('Stop World');
  if (process.env.RTWKIT_AI_SMOKE === '1') aiFixture = await (await import('./ai-story-checks.mjs')).runAiChecks({cdp,cli,query,worlds,panel,first,second,home,root,checks,retiredViews});
  await cdp.call('Browser.setDownloadBehavior',{behavior:'allow',downloadPath:root});
  await cdp.evaluate('Object.defineProperty(window,"showSaveFilePicker",{value:undefined,configurable:true});void 0');
  for (const view of await cdp.evaluate("window.__storyAdmin.rpc('ui.list')")) if (view.worldId===first.worldId) retiredViews.add(view.id);
  await cdp.click('Workspace controls');await cdp.click('Export World…');await cdp.click('Create export');await cdp.waitText('Export ready');
  assert.equal((await worlds()).find(w=>w.worldId===first.worldId).active,false,'export drains/stops source World');
  await cdp.evaluate('Object.defineProperty(window,"showSaveFilePicker",{configurable:true,value:function(){window.__savePickerReceiver=this===window;return Promise.reject(new DOMException("Cancelled","AbortError"));}});void 0');
  await cdp.click('Download .rtw');await eventually(()=>cdp.evaluate('document.querySelector("dialog[open]")?.innerText.includes("Export ready")&&!document.querySelector("dialog[open] .primary").disabled'),'cancelled save picker keeps prepared result');
  assert.equal(await cdp.evaluate('window.__savePickerReceiver'),true,'save picker is called with the Window receiver');
  await cdp.evaluate('Object.defineProperty(window,"showSaveFilePicker",{configurable:true,value:undefined});void 0');
  await cdp.click('Download .rtw');const exported=join(root,'Story A.rtw');
  await eventually(async()=>{try{return (await stat(exported)).size>0;}catch{return false;}},'complete browser RTW download');
  await eventually(()=>cdp.evaluate('!document.querySelector("dialog[open]")'),'download dialog closed');
  // Ordinary package transfer uses its own Host capability; policy RPCs stay shell-only.
  const denied = await cdp.evaluate(`(async()=>{const views=await window.__storyAdmin.rpc('ui.list');const view=views.find(v=>v.worldId===${JSON.stringify(second.worldId)}&&v.title==='Chat');const binding=await window.__storyAdmin.rpc('ui.mount',{id:view.id});try{const outcomes=[];for(const [method,args] of [['packages.begin-upload',{size:'1'}],['runtime-policy.live-component',{scopeId:'host',instanceId:'forged',componentId:'backend'}],['runtime-policy.apply-live-change',{change:{target:{},delta:{}}}],['runtime-policy.grant-http-origin',{scopeId:'host',instanceId:'forged',componentId:'backend',origin:'http://127.0.0.1:11434'}]]){try{await window.__storyAdmin.channelCall(binding.channel,method,args);outcomes.push([method,'unexpected-success']);}catch(error){outcomes.push([method,error.message]);}}return outcomes;}finally{await window.__storyAdmin.rpc('ui.unmount',{channel:binding.channel});}})()`);
  await writeFile(join(root,'ordinary-authority-denials.json'),JSON.stringify(denied,null,2));
  assert.deepEqual(denied,[['packages.begin-upload','permission-denied'],['runtime-policy.live-component','invalid-request'],['runtime-policy.apply-live-change','invalid-request'],['runtime-policy.grant-http-origin','invalid-request']]);
  await cdp.click('New World'); await cdp.setInput('#world-name','Imported Story');
  await cdp.call('Page.setInterceptFileChooserDialog',{enabled:true});let chooserCount=0;
  const observeChooser=event=>{const data=JSON.parse(event.data);if(data.method==='Page.fileChooserOpened')chooserCount++;};cdp.socket.addEventListener('message',observeChooser);
  const chooseFileOption=async()=>{const before=chooserCount;await cdp.evaluate('(()=>{const select=document.querySelector("#world-template");select.value="__file__";select.dispatchEvent(new Event("change",{bubbles:true}));})()',undefined,true);await eventually(()=>chooserCount>before,'From file opens native file chooser');};
  const chooseRootFile=async(path)=>{const input=await cdp.call('Runtime.evaluate',{expression:'document.querySelector(".template-file")'});const {node}=await cdp.call('DOM.describeNode',{objectId:input.result.objectId});await cdp.call('DOM.setFileInputFiles',{backendNodeId:node.backendNodeId,files:path?[path]:[]});await cdp.call('Runtime.releaseObject',{objectId:input.result.objectId});};
  const beforeSelection=await cdp.evaluate('document.querySelector("#world-template").value');await chooseFileOption();await chooseRootFile(null);
  assert.equal(await cdp.evaluate('document.querySelector("#world-template").value'),beforeSelection,'file picker cancellation preserves selected template');
  const corrupt=join(root,'corrupt.rtw');await writeFile(corrupt,'not an RTW');const count=(await worlds()).length;
  await chooseFileOption();await chooseRootFile(corrupt);
  await eventually(()=>cdp.evaluate('!!document.querySelector("dialog[open] [role=alert]")&&!document.querySelector("#world-template").disabled'),'corrupt RTW reports recoverable error');
  assert.equal((await worlds()).length,count,'corrupt package never publishes a World');
  await chooseFileOption();await chooseRootFile(exported);
  await eventually(()=>cdp.evaluate('document.querySelector("#world-template").selectedOptions[0].textContent.includes("Story A")&&!document.querySelector("#world-template").disabled'),'uploaded export becomes selected template',30000);
  await eventually(()=>cdp.evaluate('!!document.querySelector(".template-approval input")'),'imported template permission review');
  if(aiFixture) {
    await cdp.evaluate('document.querySelector(".template-requirements").open=true');
    await cdp.waitText(aiFixture.origin);
    const permissions=await cdp.evaluate('[...document.querySelectorAll(".template-requirements li")].map(item=>item.textContent)');
    assert.equal(permissions.filter(item=>item.includes(aiFixture.origin)).length,1);
    assert.equal(permissions.filter(item=>item.includes(aiFixture.keyPath)).length,1);
    assert.equal(permissions.filter(item=>item.includes('published-preference-write')).length,1);
    assert.equal(await cdp.evaluate('document.querySelectorAll(".template-access-summary").length'),0);
  }
  assert.equal(await cdp.evaluate('document.querySelector(".template-approval input").checked'),false);
  await cdp.evaluate('document.querySelector(".template-approval input").click()');await cdp.click('Create world');await cdp.waitText('Stop World');
  welcome=await panel('.story-welcome');await cdp.waitText('Continue your story',welcome);await cdp.click('Story Alice',welcome);
  chat=await panel('.chat');await cdp.waitText('Saved from the template workflow',chat);
  const clone=(await worlds()).find(w=>w.title==='Imported Story');assert.ok(clone&&clone.worldId!==first.worldId);
  if (aiFixture) {
    await cdp.waitText('Local deterministic reply: Привет.', chat);
    const imported = (await query(clone.worldId, 'Chat', 'rintawa/inference/v1', {method:'describe'})).value;
    assert.deepEqual(imported,{ready:true,reason:null},'same-home import resolves the baseline primary');
    const importedProfile=await readFile(join(home,'worlds',clone.worldId,'composition.toml'),'utf8');assert.ok(!importedProfile.includes('rintawa.ai.binding.v1'));assert.ok(!importedProfile.includes('rintawa.ai.connections.v1'));
    const importedView=(await cdp.evaluate("window.__storyAdmin.rpc('ui.list')")).find(v=>v.worldId===clone.worldId&&v.title==='Ai Provider');
    const importedPolicy=await cdp.evaluate(`window.__storyAdmin.rpc('runtime-policy.live-component',{scopeId:${JSON.stringify('world:'+clone.worldId)},instanceId:${JSON.stringify(importedView.restoreKey.instanceId)},componentId:'backend'})`);
    assert.deepEqual(importedPolicy.httpOrigins,[aiFixture.origin]);assert.deepEqual(importedPolicy.secretRead,[aiFixture.keyPath]);
    assert.ok(importedPolicy.granted.includes('published-preference-write'));
    const networkCount = aiFixture.requests.length;
    const importMessage = 'Imported World uses the shared primary';
    await cdp.setInput('#message',importMessage,chat);await cdp.click('Send',chat);
    await eventually(()=>cdp.evaluate(`(() => {
      const rows = [...document.querySelectorAll('.transcript .message')];
      const user = rows.at(-2);
      const assistant = rows.at(-1);
      return user?.querySelector('.message-avatar.user') && user.querySelector('.message-body p')?.textContent === ${JSON.stringify(importMessage)}
        && assistant?.querySelector('.message-avatar.assistant') && assistant.querySelector('.message-body p')?.textContent === 'Local deterministic reply: Привет.';
    })()`,chat),'same-home import displays the committed reply',30000);
    assert.equal(aiFixture.requests.length,networkCount+1);
    assert.equal(aiFixture.requests.at(-1).body.model,'updated-fixture');
    await cdp.waitText('Hello You, I am Story Alice.',chat);
    checks.push('same-home import preserves greeting and reply, explicitly reviews known permissions and uses the baseline primary without copying the catalog');
  }

  assert.equal((await query(clone.worldId,'Characters','rintawa/character/v1',{method:'list'})).value.items.length,1);
  checks.push('browser export -> native file chooser -> explicit permissions -> independent clone with preserved Character/chat/message; corrupt upload and picker cancellation recover');
  await cdp.call('Page.setInterceptFileChooserDialog',{enabled:false});cdp.socket.removeEventListener('message',observeChooser);
  for(const view of await cdp.evaluate("window.__storyAdmin.rpc('ui.list')"))if(view.worldId===clone.worldId)retiredViews.add(view.id);
  await cdp.click('Workspace controls');await cdp.click('Delete World…');await cdp.click('Delete World');
  await eventually(async()=>!(await worlds()).some(w=>w.worldId===clone.worldId),'active imported World stopped and deleted');
  assert.ok((await worlds()).some(w=>w.worldId===first.worldId),'export source retained');
  assert.equal((await worlds()).find(w=>w.worldId===second.worldId).active,true,'other World remains running');
  await cdp.openWorld('Story B');chat=await panel('.chat');await cdp.waitText('World B remains usable',chat);
  await cdp.setInput('#message','Still writable after deleting clone',chat);await cdp.click('Send',chat);await cdp.waitText('Still writable after deleting clone',chat);
  checks.push('confirmed deletion stops only its captured World; source and another running World remain intact and writable');
  if (aiFixture) {
    await cdp.evaluate('window.__storyAdmin.close()');
    await cdp.call('Page.navigate',{url:'about:blank'}); await stopHost();
    activeHome=join(root,'clean-home');
    install('web-runtime','runtime',['background-task','loopback-listen']);
    const cleanShell=install('web-ui','shell',['background-task','world-session-read','world-session-write','runtime-policy-write']);
    for(const permission of ['background-task','world-session-read'])cli('grant-runtime',cleanShell,'home',permission);
    const imported=cli('package-install',exported);const digest=/^digest = (.+)$/m.exec(imported)?.[1];assert.ok(digest);
    const created=cli('world-create','--template',digest,'--title','Clean imported story','--approve-runtime-permissions');
    const cleanId=/^world = (.+)$/m.exec(created)?.[1];assert.ok(cleanId&&cleanId!==first.worldId);
    const cleanLaunch=await startHost();cdp.rootFrame=(await cdp.call('Page.navigate',{url:cleanLaunch})).frameId;
    await cdp.waitShell();await admin();await cdp.openWorld('Clean imported story');await cdp.click('Start World');await cdp.waitText('Stop World');
    welcome=await panel('.story-welcome');await cdp.waitText('Continue your story',welcome);await cdp.click('Story Alice',welcome);
    chat=await panel('.chat');await cdp.waitText('Local deterministic reply: Привет.',chat);await cdp.waitText('Concurrent later user message',chat);
    await eventually(()=>cdp.evaluate('document.querySelector(".chat-heading img")?.naturalWidth > 0',chat),'clean-home import decodes the exported portrait');
    const info=(await query(cleanId,'Chat','rintawa/inference/v1',{method:'describe'})).value;
    assert.deepEqual(info,{ready:false,reason:'shared-connections-unavailable'});
    assert.equal((await query(cleanId,'Chat','rintawa/chat/v1',{method:'list'})).value.items[0].id,aiFixture.conversation);
    const profile=await readFile(join(activeHome,'worlds',cleanId,'composition.toml'),'utf8');assert.ok(!profile.includes('[[secret_read_grants]]'));assert.ok(!profile.includes('[[http_loopback_origins]]'));assert.ok(!profile.includes('rintawa.ai.binding.v1'));assert.ok(!profile.includes('rintawa.ai.connections.v1'));
    const count=aiFixture.requests.length;await cdp.setInput('#message','Message in clean import',chat);await cdp.click('Send',chat);await cdp.waitText('Start Ai Provider on Home',chat);assert.equal(aiFixture.requests.length,count);
    await cdp.waitText('Hello You, I am Story Alice.',chat);
    checks.push('clean-home import preserves graph, PNG, greeting and reply; generation stays unavailable until a baseline provider exists');
  }
  const expectedRetiredResource = failure => failure.status === 403 && new URL(failure.url).pathname.startsWith('/bundle/') && retiredViews.has(new URL(failure.url).pathname.split('/')[2]);
  assert.deepEqual(cdp.resourceFailures.filter(failure => !expectedRetiredResource(failure)), []);
  let deniedResources=cdp.resourceFailures.filter(expectedRetiredResource).length;
  assert.deepEqual(cdp.errors.filter(value => { if(value.includes('WebSocket connection'))return false;if(value.includes('403 (Forbidden)')&&deniedResources>0){deniedResources--;return false;}return true;}), []);
  console.log(JSON.stringify({ root, home, checks }, null, 2));
 } catch (failure) {
  if (cdp && failure.message === 'Timed out: text Stop World') {
    const methods = ['world-sessions.list-worlds', 'ui.list'];
    const results = await Promise.allSettled(methods.map(method => cdp.evaluate(`window.__storyAdmin.rpc(${JSON.stringify(method)})`)));
    const status = Object.fromEntries(results.map((result, index) => [methods[index],
      result.status === 'fulfilled' ? { value: result.value } : { error: result.reason.message }]));
    await writeFile(join(root,'world-start-timeout.json'),JSON.stringify(status,null,2));
  }
  throw failure;
 } finally {
  if(cdp) { try { await writeFile(join(root, 'browser-body.txt'), redact(await cdp.body())); const shot = await cdp.call('Page.captureScreenshot',{format:'png'});await writeFile(join(root,'last-page.png'),Buffer.from(shot.data,'base64')); }catch{} }
  if (aiFixture) {
    await aiFixture.cleanup();
    await writeFile(join(root,'ai-requests.json'),JSON.stringify(aiFixture.requests,null,2));
  }
  cdp?.socket.close(); if (chrome && chrome.exitCode === null) chrome.kill('SIGTERM'); await stopHost();
  await writeFile(join(root, 'host.log'), redact(hostOutput)); await writeFile(join(root, 'chromium.log'), redact(chromeOutput));
  await writeFile(join(root, 'checks.json'), JSON.stringify({ checks, browserErrors: cdp?.errors ?? [], worldCalls: cdp?.worldCalls ?? [], worldCommands: cdp?.worldCommands ?? [], resourceFailures: cdp?.resourceFailures ?? [] }, null, 2));
  console.log('Evidence:', root);
}
