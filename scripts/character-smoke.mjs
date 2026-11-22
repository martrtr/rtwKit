// Real browser Character import, public asset read, linked Chat and Host restart.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, readFile, writeFile, appendFile, chmod, stat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { deflateSync } from 'node:zlib';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { CDP, eventually, pause, redact } from './browser-driver.mjs';

const core = process.env.RINTAWA_BINARY, artifacts = process.env.RTWKIT_SMOKE_ARTIFACTS;
const privatePath = process.env.RTWKIT_PRIVATE_CARD;
if (!core || !artifacts) throw new Error('Set RINTAWA_BINARY and RTWKIT_SMOKE_ARTIFACTS');
const root = await mkdtemp(join(tmpdir(), 'rtwkit-character-'));
const home = join(root, 'home'), checks = [];
let host, chrome, cdp, launch, sourceRef, sourceDigest, characterId;
let hostOutput = '', chromeOutput = '';
const safe = value => { let result = redact(String(value)); if (privatePath) for (const part of [privatePath, basename(privatePath)]) if (part) result = result.replaceAll(part, '<private-card>'); return result; };
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
function cli(...args) {
  const result = spawnSync(core, ['--home', home, ...args], { encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`CLI ${args[0]} failed: ${result.stderr}`);
  return result.stdout;
}
function install(slug, permissions) {
  const output = cli('install', join(artifacts, `${slug}.rtw`));
  const id = /^instance = (.+)$/m.exec(output)?.[1]; assert.ok(id, `instance ID for ${slug}`);
  for (const [component, grants] of Object.entries(permissions)) for (const grant of grants) cli('grant-runtime', id, component, grant);
}
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) { crc ^= byte; for (let i = 0; i < 8; i++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1)); }
  return (crc ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const name = Buffer.from(type), size = Buffer.alloc(4), crc = Buffer.alloc(4);
  size.writeUInt32BE(data.length); crc.writeUInt32BE(crc32(Buffer.concat([name, data])));
  return Buffer.concat([size, name, data, crc]);
}
function card(version, name) {
  return { spec: `chara_card_v${version}`, spec_version: `${version}.0`, data: { name, description: 'Synthetic acceptance fixture', personality: '', scenario: '', first_mes: '', mes_example: '', creator_notes: '', system_prompt: '', post_history_instructions: '', alternate_greetings: [], tags: [], creator: 'test', character_version: '1', extensions: {} } };
}
function png() {
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(1, 0); ihdr.writeUInt32BE(1, 4); ihdr[8] = 8; ihdr[9] = 6;
  const metadata = Buffer.from(`ccv3\0${Buffer.from(JSON.stringify(card(3, 'Portrait Alice'))).toString('base64')}`);
  return Buffer.concat([Buffer.from('89504e470d0a1a0a', 'hex'), chunk('IHDR', ihdr), chunk('tEXt', metadata), chunk('IDAT', deflateSync(Buffer.from([0, 128, 64, 192, 255]))), chunk('IEND', Buffer.alloc(0))]);
}
async function startHost() {
  let phase = '';
  host = spawn(core, ['--home', home, 'run'], { env: { ...process.env, RUST_LOG: 'info' }, stdio: ['ignore', 'pipe', 'pipe'] });
  const consume = data => { const value = data.toString(); phase += value; hostOutput += value; };
  host.stdout.on('data', consume); host.stderr.on('data', consume);
  return eventually(() => { if (host.exitCode !== null) throw new Error('Host exited before launch'); return /http:\/\/127\.0\.0\.1:\d+\/shell\/[^\s\x1b]+/.exec(phase)?.[0]; }, 'private Host launch', 20000);
}
async function stopHost() {
  if (!host || host.exitCode !== null) return;
  host.kill('SIGINT'); await Promise.race([new Promise(resolve => host.once('exit', resolve)), pause(8000)]);
  if (host.exitCode === null) { host.kill('SIGKILL'); await new Promise(resolve => host.once('exit', resolve)); }
}
async function setValue(selector, value, context) {
  await cdp.evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e)throw Error('missing input');const proto=e instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;Object.getOwnPropertyDescriptor(proto,'value').set.call(e,${JSON.stringify(value)});e.dispatchEvent(new Event('input',{bubbles:true}));})()`, context);
}
async function setFile(path, context) {
  // Resolve within the panel's execution context, including its OOPIF CDP session.
  await cdp.call('DOM.enable', {}, context.sessionId);
  const value = await cdp.call('Runtime.evaluate', { expression: 'document.querySelector(\'input[type="file"]\')', contextId: context.id }, context.sessionId);
  assert.ok(value.result.objectId, 'Character file input');
  const { node } = await cdp.call('DOM.describeNode', { objectId: value.result.objectId }, context.sessionId);
  assert.ok(node.backendNodeId, 'Character file input backend node');
  await cdp.call('DOM.setFileInputFiles', { backendNodeId: node.backendNodeId, files: [path] }, context.sessionId);
  await cdp.call('Runtime.releaseObject', { objectId: value.result.objectId }, context.sessionId);
}
async function openPanel(title, selector) {
  await eventually(async () => { await cdp.click('Refresh'); return cdp.evaluate(`!!document.querySelector(${JSON.stringify(`button[title="Open ${title}"]`)})`); }, `${title} catalog`);
  await cdp.click(`Open ${title}`);
  return eventually(async () => (await cdp.panelContexts(selector))[0], `${title} iframe`);
}
async function importCard(panel, path, expectedName) {
  await eventually(() => cdp.evaluate('!document.querySelector(\'input[type="file"]\').disabled', panel), 'enabled Character import');
  // Observe a new completion transition, never a success notice from a prior import.
  await cdp.evaluate(`(()=>{window.__importCompletion=0;window.__importObserver?.disconnect();let cleared=false;window.__importObserver=new MutationObserver(()=>{const done=document.body.innerText.includes('Character imported.');if(!done)cleared=true;if(cleared&&done)window.__importCompletion++;});window.__importObserver.observe(document.body,{subtree:true,childList:true,characterData:true});})()`, panel);
  await setFile(path, panel);
  await eventually(() => cdp.evaluate('window.__importCompletion > 0', panel), 'new Character import completion');
  await cdp.evaluate('window.__importObserver.disconnect()', panel);
  if (expectedName) await cdp.waitText(expectedName, panel);
}
async function portraitVisible(panel) {
  return eventually(() => cdp.evaluate('(()=>{const i=document.querySelector(".character-content section img");return !!i && i.complete && i.naturalWidth>0 && i.naturalHeight>0})()', panel), 'decoded Character portrait');
}
// A second public bridge connection mounts the installed UI principal and uses its
// ordinary scoped transport and asset-read permissions; no Host-private imports.
async function inspectCharacter(targetDigest = null, excludeWorld = null) {
  return cdp.evaluate(`(async()=>{
    const ws=new WebSocket('ws://'+location.host+'/bridge');let rootChannel;let n=0;const pending=new Map();
    await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('auth timeout')),4000);ws.onopen=()=>ws.send(JSON.stringify({v:1,type:'auth',token:sessionStorage.getItem('rintawa.session')}));ws.onmessage=e=>{const v=JSON.parse(e.data);if(v.type==='ready'){clearTimeout(timer);rootChannel=v.channel;resolve();}else{const p=pending.get(v.id);if(p){pending.delete(v.id);clearTimeout(p.timer);v.ok?p.resolve(v.value):p.reject(new Error(v.error.code));}}};});
    function rpc(channel,method,args={}){const id=String(++n);return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{pending.delete(id);reject(new Error('RPC timeout'));},4000);pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({v:1,id,channel,method,args}));});}
    let mounted,reader;
    try {
      const worlds=await rpc(rootChannel,'world-sessions.list-worlds');const world=worlds.find(w=>w.active&&w.worldId!==${JSON.stringify(excludeWorld)});if(!world)throw Error('no active world');
      const view=(await rpc(rootChannel,'ui.list')).find(v=>v.title==='Characters'&&v.worldId===world.worldId);if(!view)throw Error('Character view unavailable');
      mounted=await rpc(rootChannel,'ui.mount',{id:view.id});const channel=mounted.channel;
      const encode=value=>btoa(unescape(encodeURIComponent(JSON.stringify(value))));
      async function call(method,args={}){const handle=await rpc(channel,'transport.begin-query',{point:'rintawa/character/v1',payload:encode({method,...args})});for(let i=0;i<200;i++){const replies=await rpc(channel,'transport.poll-query',{handle});if(replies!==null){if(replies.length!==1)throw Error('owner count');const result=JSON.parse(decodeURIComponent(escape(atob(replies[0]))));if(!result.ok)throw Error(result.error.code);return result.value;}await new Promise(r=>setTimeout(r,15));}throw Error('query timeout');}
      const items=[];let after;do{const page=await call('list',after?{after}:{});items.push(...page.items);after=page.next;}while(after);
      const wanted=${JSON.stringify(targetDigest)};const row=wanted?items.find(item=>item.card.source.digest==='sha256:'+wanted):null;if(!row)return {items,worldId:world.worldId};
      const card=await call('get',{itemId:row.id});reader=await rpc(channel,'asset-store.open-asset',{reference:card.card.source});const parts=[];
      for(;;){const data=await rpc(channel,'asset-store.read-asset',{handle:reader,maximumBytes:32768});if(!data.length)break;parts.push(data);}
      await rpc(channel,'asset-store.close-asset',{handle:reader});reader=null;
      return {items,card,sourceParts:parts,worldId:world.worldId};
    }finally{if(reader&&mounted)await rpc(mounted.channel,'asset-store.close-asset',{handle:reader}).catch(()=>{});if(mounted)await rpc(rootChannel,'ui.unmount',{channel:mounted.channel}).catch(()=>{});ws.close();}
  })()`);
}
async function snapshot(filename = 'screenshot.png') {
  if (!cdp) return;
  const shot = await cdp.call('Page.captureScreenshot', { format: 'png' }); await writeFile(join(root, filename), Buffer.from(shot.data, 'base64'));
}
try {
  const source = png(), pngPath = join(root, 'v3-card.png'), jsonPath = join(root, 'v2-card.json'), invalidPath = join(root, 'malformed.json');
  sourceDigest = digest(source); await writeFile(pngPath, source); await writeFile(jsonPath, JSON.stringify(card(2, 'JSON Bob'))); await writeFile(invalidPath, '{"spec":');
  install('web-runtime', { runtime: ['background-task', 'loopback-listen'] });
  install('web-ui', { shell: ['background-task', 'world-session-read', 'world-session-write'] });
  install('character', { backend: ['background-task', 'user-resource-read', 'asset-import'], ui: ['background-task', 'user-resource-import', 'asset-read'], story: ['background-task'] });
  install('chat', { backend: ['background-task'], ui: ['background-task', 'asset-read'] });
  // set_world_default adds each distinct activation; verify the generated recipe and pin both exact instances.
  cli('world-default', 'rintawa.rtwkit.character'); cli('world-default', 'rintawa.rtwkit.chat');
  const recipe = join(home, 'profiles/default-world.toml');
  const parsed = spawnSync('python3', ['-c', `import sys,tomllib,json\np=tomllib.load(open(sys.argv[1],'rb'))\nprint(json.dumps(p))`, recipe], { encoding: 'utf8' });
  if (parsed.status !== 0) throw new Error('Cannot read generated default composition');
  const profile = JSON.parse(parsed.stdout);
  for (const name of ['character', 'chat']) {
    const activations = profile.activations.filter(row => row.subject === `rintawa.rtwkit.${name}`);
    assert.equal(activations.length, 1, `one default ${name} activation`);
    assert.ok(!profile.authoritative_providers?.some(row => row.point === `rintawa/${name}/v1`), `no existing ${name} pin`);
    await appendFile(recipe, `\n[[authoritative_providers]]\nscope-id = "default-world"\npoint = "rintawa/${name}/v1"\nprovider-instance-id = "${activations[0]['instance-id']}"\nprovider-component-id = "backend"\n`);
  }
  launch = await startHost();
  chrome = spawn(process.env.CHROMIUM_BINARY ?? '/usr/bin/chromium', ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--disable-background-networking', '--disable-component-update', '--disable-sync', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${join(root, 'chromium')}`, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
  chrome.stderr.on('data', data => { chromeOutput += data.toString(); });
  const port = await eventually(async () => { try { return (await readFile(join(root, 'chromium/DevToolsActivePort'), 'utf8')).split('\n')[0]; } catch { return null; } }, 'Chromium debugger');
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  cdp = await CDP.connect(targets.find(target => target.type === 'page').webSocketDebuggerUrl);
  await cdp.call('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true });
  await cdp.call('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
  for (const method of ['Runtime.enable', 'Page.enable', 'Log.enable']) await cdp.call(method);
  cdp.rootFrame = (await cdp.call('Page.navigate', { url: launch })).frameId;
  await cdp.waitShell(); await cdp.waitText('Welcome to Rintawa'); await cdp.createEmptyWorld('Acceptance World A'); await cdp.click('Start World'); await cdp.waitText('Stop World');
  let character = await openPanel('Characters', '.character'); await cdp.waitText('Characters', character);
  await importCard(character, pngPath, 'Portrait Alice'); await portraitVisible(character);
  let inspected = await inspectCharacter(sourceDigest); assert.equal(inspected.items.length, 1); characterId = inspected.card.id; sourceRef = inspected.card.card.source; const firstWorld = inspected.worldId;
  assert.ok(inspected.card.card.portrait); assert.equal(digest(Buffer.concat(inspected.sourceParts.map(part => Buffer.from(part, 'base64')))), sourceDigest, 'exact original source bytes in CAS');
  checks.push('V3 PNG imported via Character UI, decoded portrait shown, exact source read through public scoped asset API');
  await setFile(invalidPath, character);
  await eventually(() => cdp.evaluate('!!document.querySelector(".rtw-status.error")', character), 'malformed card error');
  inspected = await inspectCharacter(); assert.equal(inspected.items.length, 1, 'invalid import created no Character');
  checks.push('malformed JSON reports an error and creates no Character');
  await importCard(character, jsonPath, 'JSON Bob'); inspected = await inspectCharacter(); assert.equal(inspected.items.length, 2);
  checks.push('V2 JSON imported through file input');
  // Real CAS publication failure consumes the native upload handle. Its stale
  // cancellation must finish cleanup so the following import can use the lane.
  const assetTemp = join(home, 'assets/tmp'), mode = (await stat(assetTemp)).mode & 0o777;
  try {
    await chmod(assetTemp, 0o500);
    await setFile(pngPath, character);
    await eventually(() => cdp.evaluate('!!document.querySelector(".rtw-status.error")', character), 'CAS publication failure becomes terminal');
    inspected = await inspectCharacter();
    assert.equal(inspected.items.length, 2, 'CAS publication failure creates no Character');
  } finally { await chmod(assetTemp, mode); }
  await importCard(character, pngPath, 'Portrait Alice');
  inspected = await inspectCharacter();
  assert.equal(inspected.items.length, 3, 'import lane recovers after CAS failure');
  checks.push('CAS failure leaves no partial Character and stale upload cleanup does not block following import');

  if (privatePath) {
    const privateBytes = await readFile(privatePath); const privateDigest = digest(privateBytes);
    assert.ok(!inspected.items.some(item => item.card.source.digest === `sha256:${privateDigest}`), 'private source must be unique in this World');
    await importCard(character, privatePath); await portraitVisible(character);
    inspected = await inspectCharacter(privateDigest);
    assert.equal(inspected.items.length, 4, 'private import adds one Character');
    assert.ok(inspected.card?.card.portrait, 'private card has a portrait');
    assert.equal(digest(Buffer.concat(inspected.sourceParts.map(part => Buffer.from(part, 'base64')))), privateDigest, 'private source matches public CAS read');
    checks.push('optional local card imported, portrait decoded and exact source read from CAS');
  }
  let chat = await openPanel('Chat', '.chat'); await cdp.openConversationDialog(chat);
  await eventually(() => cdp.evaluate(`!![...document.querySelectorAll('#character option')].find(o=>o.value===${JSON.stringify(characterId)})`, chat), 'linked Character option');
  await cdp.evaluate(`(()=>{const e=document.querySelector('#character');Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,'value').set.call(e,${JSON.stringify(characterId)});e.dispatchEvent(new Event('change',{bubbles:true}));})()`, chat);
  await setValue('#new-title', 'Character acceptance', chat); await cdp.click('Create', chat); await cdp.waitText('Message', chat);
  await eventually(() => cdp.evaluate('document.querySelector(".chat-heading")?.textContent.includes("Portrait Alice")', chat), 'linked Character header');
  await eventually(() => cdp.evaluate('(()=>{const i=document.querySelector(".chat-heading img");return !!i&&i.complete&&i.naturalWidth>0})()', chat), 'linked Chat portrait');
  await setValue('#message', 'Persisted Character chat message.', chat); await cdp.click('Send', chat); await cdp.waitText('Persisted Character chat message.', chat);
  checks.push('Chat creates a linked conversation, displays Character portrait and commits a message');
  await stopHost(); launch = await startHost(); cdp.rootFrame = (await cdp.call('Page.navigate', { url: launch })).frameId;
  await cdp.waitText('Start World'); await cdp.click('Start World'); await cdp.waitText('Stop World');
  character = await openPanel('Characters', '.character'); await cdp.waitText('Portrait Alice', character); await cdp.click('Portrait Alice', character); await portraitVisible(character);
  inspected = await inspectCharacter(sourceDigest); assert.equal(inspected.card.id, characterId); assert.deepEqual(inspected.card.card.source, sourceRef); assert.equal(digest(Buffer.concat(inspected.sourceParts.map(part => Buffer.from(part, 'base64')))), sourceDigest);
  const restartedChat = await openPanel('Chat', '.chat'); await cdp.waitText('Character acceptance', restartedChat); await cdp.selectConversation('Character acceptance', restartedChat);
  await cdp.waitText('Persisted Character chat message.', restartedChat); await eventually(() => cdp.evaluate('document.querySelector(".chat-heading")?.textContent.includes("Portrait Alice")', restartedChat), 'persisted linked Character');
  await eventually(() => cdp.evaluate('(()=>{const i=document.querySelector(".chat-heading img");return !!i&&i.complete&&i.naturalWidth>0})()', restartedChat), 'persisted Chat portrait');
  checks.push('Host restart and explicit World start preserve Character source, portrait, Chat link and message');
  await eventually(() => cdp.evaluate(`(()=>{const tab=[...document.querySelectorAll('.dv-tab')].find(e=>e.textContent.trim()==='Chat');if(!tab)return false;tab.click();return true})()`), 'show Chat tab');
  await snapshot('character-chat.png');
  await cdp.createEmptyWorld('Acceptance World B'); await cdp.click('Start World'); await cdp.waitText('Stop World');
  await eventually(async () => (await cdp.panelContexts('.character')).length === 0, 'first World Character panel closed');
  const isolated = await openPanel('Characters', '.character');
  await eventually(() => cdp.evaluate('!document.querySelector(\'input[type="file"]\').disabled', isolated), 'second World Character read');
  const second = await inspectCharacter(null, firstWorld); assert.notEqual(second.worldId, firstWorld); assert.equal(second.items.length, 0, 'second World has no Characters');
  const isolatedBody = await cdp.body(isolated); assert.ok(!isolatedBody.includes('Portrait Alice') && !isolatedBody.includes('JSON Bob'), 'second World UI has no first World Characters');
  checks.push('second World cannot read the first World Characters');
  assert.deepEqual(cdp.errors, [], 'No browser/CSP errors'); await snapshot();
  await writeFile(join(root, 'result.json'), JSON.stringify({ status: 'passed', checks, sourceDigest }, null, 2));
  console.log(JSON.stringify({ status: 'passed', checks, artifacts: root }));
} catch (error) {
  const panels = cdp ? await Promise.all((await cdp.panelContexts('.character, .chat')).map(context => cdp.body(context).catch(() => '<gone>'))) : [];
  if (!privatePath) await snapshot().catch(() => {});
  await writeFile(join(root, 'failure.json'), JSON.stringify({ error: safe(error), checks, body: cdp ? safe(await cdp.body().catch(() => '<unavailable>')) : null, panels: privatePath ? [] : panels.map(safe), browserErrors: (cdp?.errors ?? []).map(safe) }, null, 2));
  console.error(`Character smoke failed: ${safe(error)}; evidence ${root}`); process.exitCode = 1;
} finally {
  cdp?.socket.close(); if (chrome && chrome.exitCode === null) chrome.kill('SIGTERM'); await stopHost();
  await writeFile(join(root, 'host.log'), safe(hostOutput)); await writeFile(join(root, 'chromium.log'), safe(chromeOutput));
}
