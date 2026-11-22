// Actual installed-template -> Character welcome -> linked Chat -> restart flow.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, readFile, writeFile, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CDP, eventually, pause, redact } from './browser-driver.mjs';
const core = process.env.RINTAWA_BINARY, artifacts = process.env.RTWKIT_SMOKE_ARTIFACTS;
if (!core || !artifacts) throw new Error('Set RINTAWA_BINARY and RTWKIT_SMOKE_ARTIFACTS');
const root = await mkdtemp(join(tmpdir(), 'rtwkit-story-entry-')), home = join(root, 'home');
let host, chrome, cdp, hostOutput = '', chromeOutput = ''; const checks = [];
function cli(...args) { const result = spawnSync(core, ['--home', home, ...args], { encoding: 'utf8' }); assert.equal(result.status, 0, redact(result.stderr)); return result.stdout; }
function install(slug, component, grants) { const result = cli('install', join(artifacts, `${slug}.rtw`)); const id = /^instance = (.+)$/m.exec(result)?.[1]; assert.ok(id); for (const grant of grants) cli('grant-runtime', id, component, grant); }
async function startHost() {
  let output = '';
  host = spawn(core, ['--home', home, 'run'], { env: { ...process.env, RUST_LOG: 'info' }, stdio: ['ignore', 'pipe', 'pipe'] });
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
    window.__storyAdmin={rpc:(method,args)=>rpc(root,method,args),close:()=>ws.close(),query:async(worldId,component,point,call)=>{
      const views=await rpc(root,'ui.list');const matches=views.filter(v=>v.worldId===worldId&&v.title===component);if(matches.length!==1)throw Error('ambiguous test view');const binding=await rpc(root,'ui.mount',{id:matches[0].id});
      try {const handle=await rpc(binding.channel,'transport.begin-query',{point,payload:btoa(JSON.stringify(call))});for(let i=0;i<300;i++){const replies=await rpc(binding.channel,'transport.poll-query',{handle});if(replies!==null){if(replies.length!==1)throw Error('ambiguous test owner');return JSON.parse(atob(replies[0]));}await new Promise(r=>setTimeout(r,20));}throw Error('query timeout');}
      finally {await rpc(root,'ui.unmount',{channel:binding.channel});}
    }};
  })()`);
}
const query = (world, component, point, call) => cdp.evaluate(`window.__storyAdmin.query(${[world, component, point, call].map(v => JSON.stringify(v)).join(',')})`);
const worlds = () => cdp.evaluate("window.__storyAdmin.rpc('world-sessions.list-worlds')");
const card = (version, name) => ({ spec: `chara_card_v${version}`, spec_version: `${version}.0`, data: { name, description: 'Local synthetic onboarding fixture', personality: '', scenario: '', first_mes: '', mes_example: '', creator_notes: '', system_prompt: '', post_history_instructions: '', alternate_greetings: [], tags: [], creator: 'test', character_version: '1', extensions: {} } });
try {
  install('web-runtime', 'runtime', ['background-task', 'loopback-listen']);
  install('web-ui', 'shell', ['background-task', 'world-session-read', 'world-session-write']);
  cli('package-install', join(artifacts, 'TextRoleplay.rtw'));
  assert.match(cli('template-list'), /Text Roleplay/); assert.doesNotMatch(cli('list'), /rtwkit.character|rtwkit.chat/);
  const firstCard = join(root, 'alice-v3.json'), secondCard = join(root, 'bob-v2.json');
  await writeFile(firstCard, JSON.stringify(card(3, 'Story Alice'))); await writeFile(secondCard, JSON.stringify(card(2, 'Story Bob')));
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
  assert.deepEqual(cdp.errors.filter(value => !value.includes('WebSocket connection')), []);
  console.log(JSON.stringify({ root, home, checks }, null, 2));
 } finally {
  if(cdp) { try { await writeFile(join(root, 'browser-body.txt'), redact(await cdp.body())); const shot = await cdp.call('Page.captureScreenshot',{format:'png'});await writeFile(join(root,'last-page.png'),Buffer.from(shot.data,'base64')); }catch{} }
  cdp?.socket.close(); if (chrome && chrome.exitCode === null) chrome.kill('SIGTERM'); await stopHost();
  await writeFile(join(root, 'host.log'), redact(hostOutput)); await writeFile(join(root, 'chromium.log'), redact(chromeOutput));
  await writeFile(join(root, 'checks.json'), JSON.stringify({ checks, browserErrors: cdp?.errors ?? [], worldCalls: cdp?.worldCalls ?? [] }, null, 2));
  console.log('Evidence:', root);
}
