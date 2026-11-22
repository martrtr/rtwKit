// Real Host public bridge: bounded Chat history across paging and concurrent append.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { appendFile, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import net from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CDP, redact, pause } from './browser-driver.mjs';

const core = process.env.RINTAWA_BINARY, artifacts = process.env.RTWKIT_SMOKE_ARTIFACTS;
if (!core || !artifacts) throw new Error('Set RINTAWA_BINARY and RTWKIT_SMOKE_ARTIFACTS');
const root = await mkdtemp(join(tmpdir(), 'rtwkit-story-history-')), home = join(root, 'home');
const checks = []; let host, socket, chrome, cdp, logs = '', chromeLogs = '', anchorEvidence, stage = 'bootstrap', committedRows = 0, pollRetries = 0;
const safeLog = value => redact(value).replace(/http:\/\/127\.0\.0\.1:\d+\/shell\/[^\s\x1b]+/g, '<private launch URL>');
function cli(...args) {
  const result = spawnSync(core, ['--home', home, ...args], { encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`CLI ${args[0]} failed: ${result.stderr}`);
  return result.stdout;
}
function install(name, grants) {
  const output = cli('install', join(artifacts, `${name}.rtw`));
  const id = /^instance = (.+)$/m.exec(output)?.[1]; assert.ok(id, `${name} instance`);
  for (const [component, permissions] of Object.entries(grants)) for (const permission of permissions) cli('grant-runtime', id, component, permission);
}
async function eventually(action, label, timeout = 20000, interval = 30) {
  const deadline = Date.now() + timeout;
  do { const result = await action(); if (result) return result; await pause(interval); } while (Date.now() < deadline);
  throw new Error(`Timed out: ${label}`);
}
async function stopHost() {
  if (!host || host.exitCode !== null) return;
  host.kill('SIGINT'); await Promise.race([new Promise(resolve => host.once('exit', resolve)), pause(8000)]);
  if (host.exitCode === null) { host.kill('SIGKILL'); await new Promise(resolve => host.once('exit', resolve)); }
}
// Minimal RFC 6455 text client: the Origin is required by the real local bridge.
async function connect(url) {
  const target = new URL(url); socket = net.createConnection({ host: '127.0.0.1', port: Number(target.port) });
  let bytes = Buffer.alloc(0), failure, notify; const messages = [];
  const wake = () => { notify?.(); notify = undefined; };
  socket.on('error', error => { failure = error; wake(); });
  socket.on('close', () => { failure ??= new Error('WebSocket closed'); wake(); });
  socket.on('data', chunk => { bytes = Buffer.concat([bytes, chunk]); wake(); });
  await new Promise((resolve, reject) => { socket.once('connect', resolve); socket.once('error', reject); });
  const key = randomBytes(16).toString('base64');
  socket.write(`GET /bridge HTTP/1.1\r\nHost: 127.0.0.1:${target.port}\r\nOrigin: http://127.0.0.1:${target.port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: ${key}\r\n\r\n`);
  const wait = async predicate => {
    const deadline = Date.now() + 20000;
    while (!predicate()) {
      if (failure) throw failure;
      const remaining = deadline - Date.now(); if (remaining <= 0) throw new Error('WebSocket receive timed out');
      await new Promise(resolve => { const timer = setTimeout(() => { notify = undefined; resolve(); }, remaining); notify = () => { clearTimeout(timer); resolve(); }; });
    }
  };
  await wait(() => bytes.includes('\r\n\r\n')); const end = bytes.indexOf('\r\n\r\n');
  assert.match(bytes.subarray(0, end).toString(), /^HTTP\/1\.1 101 /); bytes = bytes.subarray(end + 4);
  const send = value => {
    const payload = Buffer.from(JSON.stringify(value)), mask = randomBytes(4); let header;
    if (payload.length < 126) header = Buffer.from([0x81, 0x80 | payload.length]);
    else if (payload.length <= 65535) { header = Buffer.alloc(4); header[0] = 0x81; header[1] = 0xfe; header.writeUInt16BE(payload.length, 2); }
    else throw new Error('WebSocket request too large');
    const encoded = Buffer.from(payload); for (let i = 0; i < encoded.length; i++) encoded[i] ^= mask[i % 4];
    socket.write(Buffer.concat([header, mask, encoded]));
  };
  const receive = async () => {
    await wait(() => bytes.length >= 2); const opcode = bytes[0] & 15; assert.equal(opcode, 1, 'text frame');
    let length = bytes[1] & 127, offset = 2; assert.equal(bytes[1] & 128, 0, 'unmasked server frame');
    if (length === 126) { await wait(() => bytes.length >= 4); length = bytes.readUInt16BE(2); offset = 4; }
    if (length === 127) { await wait(() => bytes.length >= 10); length = Number(bytes.readBigUInt64BE(2)); offset = 10; }
    assert.ok(length <= 1024 * 1024, 'bounded reply'); await wait(() => bytes.length >= offset + length);
    const value = JSON.parse(bytes.subarray(offset, offset + length).toString()); bytes = bytes.subarray(offset + length); return value;
  };
  send({ v: 1, type: 'auth', token: target.hash.slice(1) }); const ready = await receive(); assert.equal(ready.type, 'ready');
  let sequence = 0;
  return { rootChannel: ready.channel, token: ready.token, async rpc(channel, method, args = {}) {
    const id = String(++sequence); send({ v: 1, id, channel, method, args });
    for (;;) { const response = messages.shift() ?? await receive(); if (response.id !== id) { messages.push(response); throw new Error('Unexpected bridge response'); } if (!response.ok) { const error = new Error(`Bridge ${method}: ${response.error?.code} (${response.error?.outcome})`); error.code = response.error?.code; error.outcome = response.error?.outcome; throw error; } return response.value; }
  } };
}
try {
  install('web-runtime', { runtime: ['background-task', 'loopback-listen'] });
  install('web-ui', { shell: ['background-task', 'world-session-read', 'world-session-write'] });
  install('chat', { backend: ['background-task'], ui: ['background-task', 'asset-read'] });
  cli('world-default', 'rintawa.rtwkit.chat');
  const recipe = join(home, 'profiles/default-world.toml');
  const parsed = spawnSync('python3', ['-c', `import sys,tomllib,json\np=tomllib.load(open(sys.argv[1],'rb'))\nprint(json.dumps(p))`, recipe], { encoding: 'utf8' });
  assert.equal(parsed.status, 0, 'default composition parsed');
  const profile = JSON.parse(parsed.stdout);
  const rows = profile.activations.filter(row => row.subject === 'rintawa.rtwkit.chat');
  assert.equal(rows.length, 1);
  const id = JSON.stringify(rows[0]['instance-id']);
  assert.ok(!profile.preferred_providers?.some(row => row['contract-id'] === 'rintawa.chat'));
  assert.ok(!profile.authoritative_providers?.some(row => row.point === 'rintawa/chat/v1'));
  await appendFile(recipe, `\n[[preferred_providers]]\nscope-id = "default-world"\ncontract-id = "rintawa.chat"\ncontract-version = 1\nprovider-instance-id = ${id}\nprovider-component-id = "backend"\n\n[[authoritative_providers]]\nscope-id = "default-world"\npoint = "rintawa/chat/v1"\nprovider-instance-id = ${id}\nprovider-component-id = "backend"\n`);
  host = spawn(core, ['--home', home, 'run'], { env: { ...process.env, RUST_LOG: 'info' }, stdio: ['ignore', 'pipe', 'pipe'] });
  host.stdout.on('data', data => { logs += data.toString(); }); host.stderr.on('data', data => { logs += data.toString(); });
  const launch = await eventually(() => { if (host.exitCode !== null) throw new Error('Host exited before launch'); return /http:\/\/127\.0\.0\.1:\d+\/shell\/[^\s\x1b]+/.exec(logs)?.[0]; }, 'Host launch');
  const bridge = await connect(launch), rootChannel = bridge.rootChannel;
  async function startWorld(title) {
    const world = await bridge.rpc(rootChannel, 'world-sessions.create');
    await bridge.rpc(rootChannel, 'world-sessions.set-metadata', { worldId: world.worldId, title, description: null, cover: null });
    await bridge.rpc(rootChannel, 'world-sessions.set-active', { worldId: world.worldId, active: true });
    await eventually(async () => (await bridge.rpc(rootChannel, 'world-sessions.list-worlds')).find(row => row.worldId === world.worldId && row.active), 'World activation');
    const view = await eventually(async () => (await bridge.rpc(rootChannel, 'ui.list')).find(row => row.title === 'Chat' && row.worldId === world.worldId), 'Chat view');
    return (await bridge.rpc(rootChannel, 'ui.mount', { id: view.id })).channel;
  }
  const channel = await startWorld('History World A');
  async function callIn(channelId, method, args = {}) {
    const payload = Buffer.from(JSON.stringify({ method, ...args })).toString('base64');
    const handle = await bridge.rpc(channelId, 'transport.begin-query', { point: 'rintawa/chat/v1', payload });
    const replies = await eventually(async () => { try { const value = await bridge.rpc(channelId, 'transport.poll-query', { handle }); return value === null ? false : value; } catch (error) { if (error.code === 'unavailable' && error.outcome === 'not-started') { pollRetries++; return false; } throw error; } }, `Chat ${method}`);
    assert.equal(replies.length, 1, 'one authoritative response');
    const raw = Buffer.from(replies[0], 'base64');
    assert.ok(raw.length <= 64 * 1024, `Chat ${method} reply exceeds 64 KiB`);
    const value = JSON.parse(raw.toString());
    if (!value.ok) throw new Error(`Chat ${method}: ${value.error?.code}`);
    return value.value;
  }
  const call = (method, args = {}) => callIn(channel, method, args);
  async function finish(operation) {
    assert.equal(operation.status, 'pending');
    const result = await eventually(async () => { const value = await call('status', { operation: operation.operation }); return value.status === 'pending' ? false : value; }, 'Chat commit');
    await call('release', { operation: operation.operation });
    assert.equal(result.status, 'committed', `Chat mutation ended ${result.status}: ${result.reason ?? ''}`);
  }
  async function create(title) {
    const [conversationId] = await call('allocate', { count: 1 });
    await finish(await call('begin-create', { conversationId, title }));
    return conversationId;
  }
  async function append(conversationId, text) {
    const [messageId, entryId] = await call('allocate', { count: 2 });
    await finish(await call('begin-send', { conversationId, messageId, entryId, text }));
    return { messageId, entryId };
  }
  stage = 'create history fixture';
  const conversationId = await create('History fixture');
  stage = 'append history messages';
  const expected = [];
  for (let index = 0; index < 70; index++) { expected.push(await append(conversationId, `History message ${index}`)); committedRows++; }
  const ids = page => page.items.map(row => row.entryId);
  function checkPage(page, expectedRows) {
    assert.deepEqual(ids(page), expectedRows.map(row => row.entryId));
    assert.equal(page.head, expectedRows.at(-1)?.entryId ?? null);
    assert.equal(page.older, expectedRows.length ? expectedRows[0].entryId : null);
  }
  stage = 'native pagination';
  const recent = await call('recent', { conversationId });
  checkPage(recent, expected.slice(38));
  const second = await call('recent', { conversationId, before: recent.older });
  checkPage(second, expected.slice(6, 38));
  const later = await append(conversationId, 'Concurrent later message');
  const third = await call('recent', { conversationId, before: second.older });
  assert.deepEqual(ids(third), expected.slice(0, 6).map(row => row.entryId));
  assert.equal(third.head, expected[5].entryId);
  // A short final page signals exhaustion; older may itself be null.
  assert.equal(third.older, null);
  assert.equal(new Set([...ids(recent), ...ids(second), ...ids(third)]).size, 70);
  const forward = await call('page', { conversationId, after: recent.head });
  assert.deepEqual(ids(forward), [later.entryId]);
  checks.push('70 committed rows page newest to oldest without gaps despite a later append; forward cursor sees the append');
  stage = 'large escaped messages';
  const largeConversation = await create('Escaped history fixture');
  const largeText = '"'.repeat(16 * 1024);
  const largeRows = [];
  for (let index = 0; index < 4; index++) largeRows.push(await append(largeConversation, largeText));
  const bounded = await call('recent', { conversationId: largeConversation });
  assert.ok(bounded.items.length > 0 && bounded.items.length < 4, 'escaped messages force byte-bounded page');
  assert.deepEqual(ids(bounded), largeRows.slice(-bounded.items.length).map(row => row.entryId));
  assert.equal(bounded.head, largeRows.at(-1).entryId);
  assert.equal(bounded.older, bounded.items[0].entryId);
  checks.push('16 KiB escaped messages force a short recent page within the 64 KiB reply limit');
  stage = 'foreign World isolation';
  const foreignChannel = await startWorld('History World B');
  await assert.rejects(callIn(foreignChannel, 'recent', { conversationId }), /Chat recent: not-found/);
  checks.push('another World cannot read the conversation through its own same-point Chat channel');

  // The bridge rotates the session token at auth. Keep the replacement in memory
  // and give it to the isolated browser without printing the private launch URL.
  stage = 'browser setup';
  assert.ok(bridge.token, 'rotated browser session token');
  const browserUrl = new URL(launch); browserUrl.hash = bridge.token;
  chrome = spawn(process.env.CHROMIUM_BINARY ?? '/usr/bin/chromium', [
    '--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--disable-background-networking',
    '--disable-component-update', '--disable-sync', '--no-first-run', '--no-default-browser-check',
    '--remote-debugging-port=0', `--user-data-dir=${join(root, 'chromium')}`, 'about:blank',
  ], { stdio: ['ignore', 'ignore', 'pipe'] });
  chrome.stderr.on('data', data => { chromeLogs += data.toString(); });
  const port = await eventually(async () => { if (chrome.exitCode !== null) throw new Error('Chromium exited'); try { return (await readFile(join(root, 'chromium/DevToolsActivePort'), 'utf8')).split('\n')[0]; } catch { return false; } }, 'Chromium debugger');
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  cdp = await CDP.connect(targets.find(value => value.type === 'page').webSocketDebuggerUrl);
  await cdp.call('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true });
  await cdp.call('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
  for (const method of ['Runtime.enable', 'Page.enable', 'Log.enable']) await cdp.call(method);
  cdp.rootFrame = (await cdp.call('Page.navigate', { url: browserUrl.toString() })).frameId;
  await cdp.waitShell();
  await cdp.openWorld('History World A');
  await cdp.waitText('Stop World');
  await cdp.click('Open Chat');
  const panel = await eventually(async () => (await cdp.panelContexts('.chat'))[0], 'Chat iframe');
  stage = 'browser history pagination';
  await cdp.click('History fixture', panel);
  const messages = () => cdp.evaluate(`[...document.querySelectorAll('article.message .message-body p')].map(node => node.textContent)`, panel);
  const expectedText = Array.from({ length: 70 }, (_, index) => `History message ${index}`).concat('Concurrent later message');
  await eventually(async () => (await messages()).length === 32, 'initial bounded UI history');
  assert.deepEqual(await messages(), expectedText.slice(-32));
  // Move near the first message, then capture an actually visible message anchor.
  await cdp.evaluate(`(() => { const view = document.querySelector('.transcript'); const first = view.querySelector('article.message'); view.scrollTo({ top: first.offsetTop - view.offsetTop, behavior: 'instant' }); })()`, panel);
  const anchor = await eventually(async () => cdp.evaluate(`(() => { const view = document.querySelector('.transcript'); const bounds = view.getBoundingClientRect(); const row = [...view.querySelectorAll('article.message')].find(item => { const rect = item.getBoundingClientRect(); return rect.top >= bounds.top && rect.bottom <= bounds.bottom; }); return row && { text: row.querySelector('.message-body p').textContent, y: row.getBoundingClientRect().top }; })()`, panel), 'visible history anchor');
  anchorEvidence = { original: anchor };
  await cdp.click('Load older messages', panel);
  await eventually(async () => (await messages()).length === 64, 'first older UI page');
  assert.deepEqual(await messages(), expectedText.slice(-64));
  await eventually(async () => { anchorEvidence.current = await cdp.evaluate(`(() => { const view = document.querySelector('.transcript'); const row = [...view.querySelectorAll('article.message')].find(item => item.querySelector('.message-body p').textContent === ${JSON.stringify(anchor.text)}); return { y: row.getBoundingClientRect().top, scrollTop: view.scrollTop, scrollHeight: view.scrollHeight, clientHeight: view.clientHeight }; })()`, panel); return Math.abs(anchorEvidence.current.y - anchor.y) <= 3; }, 'restored scroll anchor');
  await cdp.click('Load older messages', panel);
  await eventually(async () => (await messages()).length === 71, 'final older UI page');
  assert.deepEqual(await messages(), expectedText);
  assert.equal(await cdp.evaluate(`!![...document.querySelectorAll('button')].find(button => button.textContent.trim() === 'Load older messages')`, panel), false);
  checks.push('actual Chat UI initially renders 32 latest messages, prepends 32 then 7 in order and preserves a visible scroll anchor');
  await bridge.rpc(rootChannel, 'ui.unmount', { channel: foreignChannel });
  await bridge.rpc(rootChannel, 'ui.unmount', { channel });
  await writeFile(join(root, 'result.json'), JSON.stringify({ status: 'passed', pollRetries, checks }, null, 2));
  console.log(JSON.stringify({ status: 'passed', checks, artifacts: root }));
} catch (error) {
  await writeFile(join(root, 'failure.json'), JSON.stringify({ error: redact(String(error)), stage, committedRows, pollRetries, checks, anchorEvidence }, null, 2));
  console.error(`Story history smoke failed: ${redact(String(error))}; evidence ${root}`); process.exitCode = 1;
} finally {
  cdp?.socket.close(); if (chrome && chrome.exitCode === null) { chrome.kill('SIGTERM'); await Promise.race([new Promise(resolve => chrome.once('exit', resolve)), pause(8000)]); if (chrome.exitCode === null) chrome.kill('SIGKILL'); }
  socket?.destroy(); await stopHost(); await writeFile(join(root, 'host.log'), safeLog(logs)); await writeFile(join(root, 'chromium.log'), safeLog(chromeLogs));
}
