// Real Host + Chromium acceptance. Only isolated temporary homes are modified.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { CDP, eventually, pause, redact } from './browser-driver.mjs';

const core = process.env.RINTAWA_BINARY;
const artifacts = process.env.RTWKIT_SMOKE_ARTIFACTS;
if (!core || !artifacts) throw new Error('Set RINTAWA_BINARY and RTWKIT_SMOKE_ARTIFACTS (directory with three built RTWs)');
const root = await mkdtemp(join(tmpdir(), 'rtwkit-browser-'));
const home = join(root, 'home'); const checks = [];
function cli(...args) {
  const result = spawnSync(core, ['--home', home, ...args], { encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`CLI ${args[0]} failed: ${result.stderr}`);
  return result.stdout;
}
function install(slug, component, permissions) {
  const result = cli('install', join(artifacts, `${slug}.rtw`));
  const instance = /^instance = (.+)$/m.exec(result)?.[1]; assert.ok(instance, 'Installed instance ID');
  for (const permission of permissions) cli('grant-runtime', instance, component, permission);
}
install('web-runtime', 'runtime', ['background-task', 'loopback-listen']);
install('web-ui', 'shell', ['background-task', 'world-session-read', 'world-session-write']);
install('dice', 'dice', ['background-task']);
cli('world-default', 'rintawa.rtwkit.dice');
let host, chrome, cdp; let hostOutput = ''; let chromeOutput = ''; let launch;
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
try {
  launch = await startHost();
  chrome = spawn(process.env.CHROMIUM_BINARY ?? '/usr/bin/chromium', ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--disable-background-networking', '--disable-component-update', '--disable-sync', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${join(root, 'chromium')}`, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
  chrome.stderr.on('data', data => { chromeOutput += data.toString(); });
  const port = await eventually(async () => { try { return (await readFile(join(root, 'chromium/DevToolsActivePort'), 'utf8')).split('\n')[0]; } catch { return null; } }, 'Chromium debugger');
  const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  cdp = await CDP.connect(pages.find(page => page.type === 'page').webSocketDebuggerUrl);
  await cdp.call('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true }); await cdp.call('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false }); await cdp.call('Network.enable'); await cdp.call('Runtime.enable'); await cdp.call('Page.enable'); await cdp.call('Log.enable');
  const navigation = await cdp.call('Page.navigate', { url: launch }); cdp.rootFrame = navigation.frameId;
  await cdp.waitShell(); await cdp.waitText('Welcome to Rintawa');
  assert.equal(await cdp.evaluate('getComputedStyle(document.body).backgroundColor'), 'rgb(0, 0, 0)', 'AMOLED background');
  assert.equal(await cdp.evaluate(`(()=>{const i=document.querySelector('button[aria-label="Home"] img');return !!i&&i.complete&&i.naturalWidth>0&&new URL(i.src).origin===location.origin})()`), true, 'local logo decoded');
  assert.equal(await cdp.evaluate('location.hash'), '', 'Bootstrap fragment removed');
  assert.equal(await cdp.evaluate('typeof sessionStorage.getItem("rintawa.session") === "string"'), true);
  checks.push('actual React shell and Root authentication');
  await cdp.createEmptyWorld('Acceptance World A');
  assert.equal(await cdp.click('Start World'), true); await cdp.waitText('Stop World');
  assert.equal(await cdp.click('Open Dice'), true);
  const first = await eventually(async () => (await cdp.panelContexts())[0], 'first Dice iframe');
  await cdp.waitText('Contribution ready', first);
  assert.equal(await cdp.evaluate('document.querySelector("iframe").getAttribute("sandbox")'), 'allow-scripts');
  assert.equal(await cdp.evaluate('(() => { try { void parent.document.body; return false; } catch { return true; } })()', first), true, 'Opaque iframe cannot read parent DOM');
  assert.equal(await cdp.evaluate('(() => { try { void sessionStorage.length; return false; } catch { return true; } })()', first), true, 'Opaque iframe has no Root storage access');
  checks.push('real opaque iframe module load and MessagePort handshake');
  await cdp.click('Workspace controls');
  assert.equal(await cdp.click('Split Dice'), true);
  const second = await eventually(async () => { const contexts = await cdp.panelContexts(); return contexts.length === 2 && contexts.find(id => id !== first); }, 'second Dice iframe');
  await cdp.waitText('Another panel provides this contribution.', second);
  await cdp.click('Ask contributions', second); await cdp.waitText('dice.roll', second);
  checks.push('two panels share activation and exchange D8 query/reply');
  const sash = await cdp.evaluate(`(() => { const element = [...document.querySelectorAll('.dv-sash')].find(e => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0; }); if (!element) return null; const r = element.getBoundingClientRect(); return { x: r.x + r.width/2, y: r.y + r.height/2 }; })()`);
  assert.ok(sash, 'Split has a real draggable sash');
  const widthBefore = await cdp.evaluate('document.querySelector("iframe").getBoundingClientRect().width');
  await cdp.call('Input.dispatchMouseEvent', { type: 'mousePressed', ...sash, button: 'left', clickCount: 1 });
  await cdp.call('Input.dispatchMouseEvent', { type: 'mouseMoved', x: sash.x + 80, y: sash.y, buttons: 1 });
  await cdp.call('Input.dispatchMouseEvent', { type: 'mouseReleased', x: sash.x + 80, y: sash.y, button: 'left', clickCount: 1 });
  await eventually(async () => Math.abs(await cdp.evaluate('document.querySelector("iframe").getBoundingClientRect().width') - widthBefore) > 30, 'real split resize');
  checks.push('Dockview split resizes through real browser pointer input');
  const before = await cdp.evaluate('Array.from(document.querySelector("canvas").getContext("2d").getImageData(0,0,1,1).data)', first);
  await cdp.click('Settings');await cdp.click('Appearance');await cdp.evaluate('(()=>{const e=document.querySelector("#settings-theme");e.value="light";e.dispatchEvent(new Event("change",{bubbles:true}));})()');await cdp.click('Close dialog');
  await eventually(async () => JSON.stringify(await cdp.evaluate('Array.from(document.querySelector("canvas").getContext("2d").getImageData(0,0,1,1).data)', first)) !== JSON.stringify(before), 'Canvas theme update');
  checks.push('theme reaches Canvas through bound MessagePort');
  await cdp.waitText('Layout saved');
  await cdp.call('Page.reload'); await cdp.waitShell();
  await eventually(async () => (await cdp.panelContexts()).length === 2, 'two restored panels after reload');
  await eventually(async () => { const contexts = await cdp.panelContexts(); return (await Promise.all(contexts.map(id => cdp.body(id)))).some(text => text.includes('Contribution ready')); }, 'restored channel ready');
  checks.push('session reauthentication and saved Dockview layout restoration');
  await cdp.createEmptyWorld('Acceptance World B');
  await eventually(async () => await cdp.evaluate('document.querySelectorAll("iframe").length') === 0, 'old World frames unmounted');
  await cdp.waitText('Another World');
  await cdp.click('Start World'); await cdp.waitText('Stop World'); await cdp.click('Open Dice');
  const worldB = await eventually(async () => (await cdp.panelContexts())[0], 'World B Dice iframe');
  await cdp.waitText('Contribution ready', worldB); await cdp.click('Ask contributions', worldB); await cdp.waitText('dice.roll', worldB);
  checks.push('World switch releases old frames and binds new World independently');
  await cdp.click('Stop World'); await cdp.waitText('Start World');
  await eventually(async () => await cdp.evaluate('document.querySelectorAll("iframe").length') === 0, 'stopped component frames released');
  await cdp.click('Start World'); await cdp.waitText('Stop World');
  const restarted = await eventually(async () => (await cdp.panelContexts())[0], 'restarted component restores stable panel');
  await cdp.waitText('Contribution ready', restarted);
  checks.push('stop and reactivation recreate channels under stable panel identity');
  await cdp.evaluate('location.reload()', restarted);
  await eventually(async () => await cdp.evaluate('document.querySelectorAll("iframe").length') === 0, 'navigated iframe channel retired');
  await cdp.waitText('View disconnected'); await cdp.click('Reopen view');
  const reopened = await eventually(async () => (await cdp.panelContexts())[0], 'fresh channel after iframe navigation');
  await cdp.waitText('Contribution ready', reopened);
  checks.push('iframe navigation retires its channel and explicit reopen creates a fresh binding');
  // Global postMessage is never an API entry point, including from a real panel.
  const worldsBefore = await cdp.evaluate('document.querySelectorAll(".world-button").length');
  const worldListReadsBefore = cdp.worldCommands.filter(call => call.method === 'world-sessions.list-worlds').length;
  await cdp.evaluate('parent.postMessage({v:1,id:"forged",method:"world-sessions.create",args:{}}, new URL(location.href).origin)', reopened);
  await eventually(() => cdp.worldCommands.filter(call => call.method === 'world-sessions.list-worlds').length > worldListReadsBefore, 'normal Home catalog reread after rejected panel message');
  assert.equal(await cdp.evaluate('document.querySelectorAll(".world-button").length'), worldsBefore);
  checks.push('panel global postMessage cannot invoke Root World operations');
  await stopHost();
  launch = await startHost();
  const restartedNavigation = await cdp.call('Page.navigate', { url: launch }); cdp.rootFrame = restartedNavigation.frameId;
  await cdp.waitText('Start World');
  await eventually(async () => await cdp.evaluate('document.querySelectorAll("iframe").length') === 0, 'inactive World placeholders after Host restart');
  await cdp.click('Start World'); await cdp.waitText('Stop World');
  await eventually(async () => (await cdp.panelContexts()).length === 2, 'stable saved panels restored after Host restart');
  await eventually(async () => { const contexts = await cdp.panelContexts(); return (await Promise.all(contexts.map(context => cdp.body(context)))).some(text => text.includes('Contribution ready')); }, 'fresh runtime service after Host restart');
  checks.push('Host restart preserves layout and exact instance keys; explicit World activation restores panels');
  const screenshot = await cdp.call('Page.captureScreenshot', { format: 'png' });
  await writeFile(join(root, 'workspace.png'), Buffer.from(screenshot.data, 'base64'));
  assert.deepEqual(cdp.errors, [], 'No browser runtime/CSP errors');
  await writeFile(join(root, 'result.json'), JSON.stringify({ status: 'passed', checks, node: process.version }, null, 2));
  console.log(JSON.stringify({ status: 'passed', checks, artifacts: root }));
} catch (error) {
  const state = cdp ? await cdp.body().catch(() => '<unavailable>') : '<no browser>';
  await writeFile(join(root, 'failure.json'), JSON.stringify({ error: redact(String(error)), checks, body: state, panelBodies: cdp ? await Promise.all((await cdp.panelContexts()).map(context => cdp.body(context).catch(() => '<gone>'))) : [], browserErrors: cdp?.errors ?? [], worldCalls: cdp?.worldCalls ?? [] }, null, 2));
  console.error(`Browser smoke failed: ${redact(String(error))}; evidence ${root}`); process.exitCode = 1;
} finally {
  cdp?.socket.close(); cdp?.socket.unref?.();
  if (chrome && chrome.exitCode === null) chrome.kill('SIGTERM');
  await stopHost();
  await writeFile(join(root, 'host.log'), redact(hostOutput));
  await writeFile(join(root, 'chromium.log'), redact(chromeOutput));
}
