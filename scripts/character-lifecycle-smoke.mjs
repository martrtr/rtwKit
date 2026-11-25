// Real Host public bridge: asset-store failure terminates Character import and permits recovery.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { chmod, mkdtemp, open, rm, stat, writeFile } from 'node:fs/promises';
import net from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { redact, pause } from './browser-driver.mjs';

const core = process.env.RINTAWA_BINARY, artifacts = process.env.RTWKIT_SMOKE_ARTIFACTS;
if (!core || !artifacts) throw new Error('Set RINTAWA_BINARY and RTWKIT_SMOKE_ARTIFACTS');
const root = await mkdtemp(join(tmpdir(), 'rtwkit-character-lifecycle-')), home = join(root, 'home');
const checks = []; let host, socket, logs = '', mode, blockedDir, failedStatus;
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
  return { rootChannel: ready.channel, async rpc(channel, method, args = {}) {
    const id = String(++sequence); send({ v: 1, id, channel, method, args });
    for (;;) { const response = messages.shift() ?? await receive(); if (response.id !== id) { messages.push(response); throw new Error('Unexpected bridge response'); } if (!response.ok) throw new Error(`Bridge ${method}: ${response.error?.code}`); return response.value; }
  } };
}
try {
  install('web-runtime', { runtime: ['background-task', 'loopback-listen'] });
  install('web-ui', { shell: ['background-task', 'world-session-read', 'world-session-write'] });
  install('character', { backend: ['background-task', 'user-resource-read', 'asset-import'], ui: ['background-task', 'user-resource-import', 'asset-read'], story: ['background-task'] });
  install('chat', { backend: ['background-task'], ui: ['background-task', 'asset-read'] });
  cli('world-default', 'rintawa.rtwkit.character'); cli('world-default', 'rintawa.rtwkit.chat');
  const recipe = join(home, 'profiles/default-world.toml');
  const parsed = spawnSync('python3', ['-c', `import sys,tomllib,json\np=tomllib.load(open(sys.argv[1],'rb'))\nprint(json.dumps(p))`, recipe], { encoding: 'utf8' });
  assert.equal(parsed.status, 0, 'default composition parsed'); const profile = JSON.parse(parsed.stdout);
  const pins = [];
  for (const name of ['character', 'chat']) {
    const rows = profile.activations.filter(row => row.subject === `rintawa.rtwkit.${name}`); assert.equal(rows.length, 1);
    assert.ok(!profile.authoritative_providers?.some(row => row.point === `rintawa/${name}/v1`));
    pins.push(`\n[[authoritative_providers]]\nscope-id = "default-world"\npoint = "rintawa/${name}/v1"\nprovider-instance-id = ${JSON.stringify(rows[0]['instance-id'])}\nprovider-component-id = "backend"\n`);
  }
  const { appendFile } = await import('node:fs/promises'); await appendFile(recipe, pins.join(''));
  host = spawn(core, ['--home', home, 'run'], { env: { ...process.env, RUST_LOG: 'info' }, stdio: ['ignore', 'pipe', 'pipe'] });
  host.stdout.on('data', data => { logs += data.toString(); }); host.stderr.on('data', data => { logs += data.toString(); });
  const launch = await eventually(() => { if (host.exitCode !== null) throw new Error('Host exited before launch'); return /http:\/\/127\.0\.0\.1:\d+\/shell\/[^\s\x1b]+/.exec(logs)?.[0]; }, 'Host launch');
  const bridge = await connect(launch), rootChannel = bridge.rootChannel;
  const world = await bridge.rpc(rootChannel, 'world-sessions.create');
  await bridge.rpc(rootChannel, 'world-sessions.set-active', { worldId: world.worldId, active: true });
  await eventually(async () => (await bridge.rpc(rootChannel, 'world-sessions.list-worlds')).find(row => row.worldId === world.worldId && row.active), 'World activation');
  const view = await eventually(async () => (await bridge.rpc(rootChannel, 'ui.list')).find(row => row.title === 'Characters' && row.worldId === world.worldId), 'Character view');
  const channel = (await bridge.rpc(rootChannel, 'ui.mount', { id: view.id })).channel;
  const call = async (method, args = {}, raw = null) => {
    const payload = Buffer.from(raw ?? JSON.stringify({ method, ...args })).toString('base64');
    const handle = await bridge.rpc(channel, 'transport.begin-query', { point: 'rintawa/character/v1', payload });
    const replies = await eventually(async () => { const value = await bridge.rpc(channel, 'transport.poll-query', { handle }); return value === null ? false : value; }, `Character ${method}`);
    assert.equal(replies.length, 1); const value = JSON.parse(Buffer.from(replies[0], 'base64').toString());
    if (!value.ok) throw new Error(`Character ${method}: ${value.error?.code}`); return value.value;
  };
  const source = Buffer.from(JSON.stringify({ spec: 'chara_card_v2', spec_version: '2.0', data: { name: 'Lifecycle fixture', description: '', personality: '', scenario: '', first_mes: '', mes_example: '' } }));
  const upload = async () => {
    const handle = await bridge.rpc(channel, 'user-resources.begin-upload', { mediaType: 'application/json', name: 'fixture.json', expectedSize: String(source.length) });
    await bridge.rpc(channel, 'user-resources.append-upload', { handle, bytes: source.toString('base64') });
    return bridge.rpc(channel, 'user-resources.finish-upload', { handle });
  };
  const [firstId] = await call('allocate', { count: 1 }); const firstResource = await upload();
  blockedDir = join(home, 'assets', 'tmp'); mode = (await stat(blockedDir)).mode & 0o777;
  await chmod(blockedDir, 0o500);
  // Prove that the running user cannot bypass this fault; never mistake a privileged run for a test.
  let blocked = false; const probe = join(blockedDir, 'permission-probe');
  try { const file = await open(probe, 'wx'); await file.close(); await rm(probe); }
  catch (error) { if (error.code === 'EACCES' || error.code === 'EPERM') blocked = true; else throw error; }
  assert.ok(blocked, 'asset temp directory must reject writes');
  const first = await call('begin-import', { itemId: firstId, resource: firstResource });
  assert.equal(first.status, 'pending'); const deadline = Date.now() + 2500;
  do { failedStatus = await call('status', { operation: first.operation }); if (failedStatus.status !== 'pending') break; await pause(40); } while (Date.now() < deadline);
  assert.equal(failedStatus.status, 'failed', `asset failure must terminate, got ${failedStatus.status}`);
  await call('release', { operation: first.operation });
  const before = await call('list'); assert.ok(!before.items.some(row => row.id === firstId), 'failed import created no Character');
  checks.push('asset-store write failure yields terminal failed status and no Character');
  await chmod(blockedDir, mode); blockedDir = null;
  const [secondId] = await call('allocate', { count: 1 }); const secondResource = await upload();
  const second = await call('begin-import', { itemId: secondId, resource: secondResource });
  const committed = await eventually(async () => { const result = await call('status', { operation: second.operation }); return result.status === 'pending' ? false : result; }, 'recovery import');
  assert.equal(committed.status, 'committed'); await call('release', { operation: second.operation });
  const card = await call('get', { itemId: secondId }); assert.equal(card.id, secondId); assert.equal(card.card.name, 'Lifecycle fixture');
  checks.push('a fresh import commits after asset-store access is restored');
  await assert.rejects(call('get', { itemId: secondId.replaceAll('-', '') }), /invalid-request/);
  await assert.rejects(call('get', {}, `{"method":"get","itemId":"${secondId}","itemId":"${secondId}"}`), /invalid-request/);
  assert.equal((await call('get', { itemId: secondId })).id, secondId);
  checks.push('canonical IDs are required and duplicate JSON fields are rejected');
  await bridge.rpc(rootChannel, 'ui.unmount', { channel });
  await writeFile(join(root, 'result.json'), JSON.stringify({ status: 'passed', checks }, null, 2));
  console.log(JSON.stringify({ status: 'passed', checks, artifacts: root }));
} catch (error) {
  await writeFile(join(root, 'failure.json'), JSON.stringify({ error: redact(String(error)), checks, failedStatus }, null, 2));
  console.error(`Character lifecycle failed: ${redact(String(error))}; evidence ${root}`); process.exitCode = 1;
} finally {
  if (blockedDir && mode !== undefined) await chmod(blockedDir, mode).catch(() => {});
  socket?.destroy(); await stopHost(); await writeFile(join(root, 'host.log'), redact(logs));
}
