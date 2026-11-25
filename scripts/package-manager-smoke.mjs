// Real Chromium/WASM acceptance for the local administrative package extension.
import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,basename} from 'node:path';
import {CDP,eventually,pause,redact} from './browser-driver.mjs';
import {createHash} from 'node:crypto';
const core=process.env.RINTAWA_BINARY,artifacts=process.env.RTWKIT_SMOKE_ARTIFACTS;
if(!core||!artifacts)throw Error('Set RINTAWA_BINARY and RTWKIT_SMOKE_ARTIFACTS');
const root=await mkdtemp(join(tmpdir(),'rtwkit-packages-')),home=join(root,'home'),checks=[],timings=[];
let host,chrome,cdp,hostOutput='',chromeOutput='';
const cli=(...args)=>{const r=spawnSync(core,['--home',home,...args],{encoding:'utf8'});assert.equal(r.status,0,redact(r.stderr));return r.stdout;};
const install=(slug,permissions)=>{const output=cli('install',join(artifacts,slug+'.rtw'));const instance=/^instance = (.+)$/m.exec(output)[1];for(const [component,grants]of Object.entries(permissions))for(const grant of grants)cli('grant-runtime',instance,component,grant);return instance;};
const panel=selector=>eventually(async()=>(await cdp.panelContexts(selector))[0],selector,30000);
async function upload(file,context){await cdp.call('DOM.enable',{},context.sessionId);const result=await cdp.call('Runtime.evaluate',{expression:'document.querySelector("input[type=file]")',contextId:context.id},context.sessionId);const {node}=await cdp.call('DOM.describeNode',{objectId:result.result.objectId},context.sessionId);await cdp.call('DOM.setFileInputFiles',{backendNodeId:node.backendNodeId,files:[file]},context.sessionId);await cdp.call('Runtime.releaseObject',{objectId:result.result.objectId},context.sessionId);}
try{
 install('web-runtime',{runtime:['background-task','loopback-listen']});
 install('web-ui',{shell:['background-task','world-session-read','world-session-write','runtime-policy-write'],home:['background-task','world-session-read']});
 install('settings-ui',{settings:['background-task'],appearance:['background-task']});
 const manager=install('package-manager',{backend:['background-task','composition-read','composition-write','runtime-policy-read','world-session-read','world-session-write'],ui:['background-task']});
 const world=/^world = (.+)$/m.exec(cli('world-create'))[1];
 const originalWorld=await readFile(join(home,'worlds',world,'world.sqlite'));
 // Two valid local versions with the same extension identity, produced from an
 // ordinary package. The publishing catalog is deliberately not involved.
 const version=join(root,'dice-v2.rtw');const py=spawnSync('python3',['-c',`import sys,zipfile
with zipfile.ZipFile(sys.argv[1]) as src,zipfile.ZipFile(sys.argv[2],'w',zipfile.ZIP_DEFLATED) as dst:
 for name in src.namelist():
  data=src.read(name)
  if name=='manifest.toml': data=data.replace(b'version = "0.1.0"',b'version = "0.2.0"')
  dst.writestr(name,data)
`,join(artifacts,'dice.rtw'),version],{encoding:'utf8'});assert.equal(py.status,0,py.stderr);
 const sameVersion=join(root,'dice-same-version.rtw');const repacked=spawnSync('python3',['-c',`import sys,zipfile
with zipfile.ZipFile(sys.argv[1]) as src,zipfile.ZipFile(sys.argv[2],'w',zipfile.ZIP_DEFLATED) as dst:
 for name in src.namelist(): dst.writestr(name,src.read(name))
 dst.writestr('local-note.txt',b'Distinct local bytes with the same declared version')
`,join(artifacts,'dice.rtw'),sameVersion],{encoding:'utf8'});assert.equal(repacked.status,0,repacked.stderr);
 const originalDigest='sha256:'+createHash('sha256').update(await readFile(join(artifacts,'dice.rtw'))).digest('hex');
 host=spawn(core,['--home',home,'run'],{env:{...process.env,RUST_LOG:'info'},stdio:['ignore','pipe','pipe']});for(const stream of[host.stdout,host.stderr])stream.on('data',b=>hostOutput+=b);
 const launch=await eventually(()=>{if(host.exitCode!==null)throw Error('Host exited');return /http:\/\/127\.0\.0\.1:\d+\/shell\/[^\s\x1b]+/.exec(hostOutput)?.[0];},'Host launch',30000);
 chrome=spawn('/usr/bin/chromium',['--headless=new','--no-sandbox','--disable-dev-shm-usage','--disable-background-networking','--disable-component-update','--no-first-run','--no-default-browser-check','--remote-debugging-port=0',`--user-data-dir=${join(root,'chromium')}`,'about:blank'],{stdio:['ignore','ignore','pipe']});chrome.stderr.on('data',b=>chromeOutput+=b);
 const port=await eventually(async()=>{try{return(await readFile(join(root,'chromium/DevToolsActivePort'),'utf8')).split('\n')[0];}catch{return null;}},'Chrome debugger');const targets=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();cdp=await CDP.connect(targets.find(t=>t.type==='page').webSocketDebuggerUrl);
 await cdp.call('Target.setAutoAttach',{autoAttach:true,waitForDebuggerOnStart:false,flatten:true});for(const method of['Runtime.enable','Page.enable','Network.enable','Log.enable'])await cdp.call(method);await cdp.call('Emulation.setDeviceMetricsOverride',{width:1280,height:900,deviceScaleFactor:1,mobile:false});cdp.rootFrame=(await cdp.call('Page.navigate',{url:launch})).frameId;await cdp.waitShell();
 await cdp.click('Packages');const packages=await panel('.packages');await cdp.waitText('Local library',packages);
 await eventually(()=>cdp.evaluate('!document.querySelector("input[type=file]").disabled',packages),'manager ready');
 assert.equal(await cdp.evaluate('!!document.querySelector(".packages")'),false);
 checks.push('Packages opens as an isolated full-size Home app, outside the Settings dialog');assert.equal(await cdp.evaluate('!!document.querySelector(".settings-overlay")'),false);
 // Retain a second channel to test service caller/transfer ownership directly.
 await cdp.evaluate(`(async()=>{const ws=new WebSocket('ws://'+location.host+'/bridge');let root,n=0;const pending=new Map();await new Promise((resolve,reject)=>{ws.onopen=()=>ws.send(JSON.stringify({v:1,type:'auth',token:sessionStorage.getItem('rintawa.session')}));ws.onerror=reject;ws.onmessage=e=>{const v=JSON.parse(e.data);if(v.type==='ready'){root=v.channel;resolve();}else{const p=pending.get(v.id);if(p){pending.delete(v.id);clearTimeout(p.timer);v.ok?p.resolve(v.value):p.reject(Error(v.error.code));}}};});const rpc=(channel,method,args={})=>new Promise((resolve,reject)=>{const id=String(++n),timer=setTimeout(()=>reject(Error('rpc timeout')),8000);pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({v:1,id,channel,method,args}));});const views=await rpc(root,'ui.list');const manager=views.find(v=>v.title==='Packages'&&v.worldId===null),other=views.find(v=>v.title==='Appearance'&&v.worldId===null);const bound=await rpc(root,'ui.mount',{id:manager.id}),foreign=await rpc(root,'ui.mount',{id:other.id});const query=async(channel,call)=>{const handle=await rpc(channel,'transport.begin-query',{point:'rintawa/packages/v1',payload:btoa(JSON.stringify(call))});for(let i=0;i<300;i++){const rows=await rpc(channel,'transport.poll-query',{handle});if(rows!==null){if(rows.length!==1)throw Error('ambiguous response');return JSON.parse(atob(rows[0]));}await new Promise(r=>setTimeout(r,20));}throw Error('query timeout');};window.__packagesTest={rpc:(method,args)=>rpc(root,method,args),call:call=>query(bound.channel,call),foreign:call=>query(foreign.channel,call)};})()`);
 const call=async body=>{const start=performance.now();const result=await cdp.evaluate(`window.__packagesTest.call(${JSON.stringify(body)})`);timings.push({method:body.method,milliseconds:performance.now()-start});return result;};
 const rpc=(method,args={})=>cdp.evaluate(`window.__packagesTest.rpc(${JSON.stringify(method)},${JSON.stringify(args)})`);
 const inventoryPage=async()=>(await call({method:'inventory'})).value;
 const foreign=await cdp.evaluate('window.__packagesTest.foreign({method:"select",scopeId:"host",digest:"sha256:"+"a".repeat(64)})');assert.equal(foreign.error.code,'access-denied');
 checks.push('an unrelated Settings component cannot invoke the manager administrative API');
 const imported=async file=>{await upload(file,packages);await cdp.waitText('Imported '+basename(file),packages);await eventually(()=>cdp.evaluate('!document.querySelector("input[type=file]").disabled',packages),'import terminal');};
 await imported(join(artifacts,'dice.rtw'));assert.equal((await call({method:'selections',scopeId:'host'})).value.some(r=>r.subject==='rintawa.rtwkit.dice'),false);
 await imported(version);await imported(sameVersion);
 let inventory=await inventoryPage();const versions=inventory.items.filter(r=>r.subject==='rintawa.rtwkit.dice');assert.equal(versions.length,3);assert.deepEqual(versions.map(r=>r.version).sort(),['0.1.0','0.1.0','0.2.0']);assert.notEqual(versions[0].digest,versions[1].digest);
 const v1=versions.find(v=>v.digest===originalDigest),same=versions.find(v=>v.version==='0.1.0'&&v.digest!==originalDigest),v2=versions.find(v=>v.version==='0.2.0');
 checks.push('bounded UI uploads keep two versions and distinct same-version bytes without selecting or executing any');
 await cdp.setInput('input[aria-label="Search packages"]','Dice',packages);
 assert.equal(await cdp.evaluate('document.querySelectorAll(".package-table tbody tr").length',packages),1);
 await cdp.evaluate("document.querySelector('.package-table tbody tr').click()",packages,true);
 await eventually(()=>cdp.evaluate(`document.querySelectorAll('select[aria-label="Package version"] option').length===3`,packages),'all cached builds offered');
 await writeFile(join(root,'launcher-library.png'),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
 await cdp.setInput('input[aria-label="Search packages"]','',packages);
 await cdp.evaluate("(()=>{const row=document.querySelector('.package-table tbody tr');row.focus();row.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}));})()",packages,true);
 await eventually(()=>cdp.evaluate('document.querySelectorAll(".package-table tbody tr")[1].getAttribute("aria-selected")==="true"',packages),'keyboard selection');
 checks.push('search groups cached builds into one package; version picker keeps every digest; keyboard moves the selected row');
 const idle=()=>eventually(()=>cdp.evaluate('!document.querySelector(".packages-status .working")',packages),'manager idle');
 const scope=async id=>{await idle();await cdp.evaluate(`(()=>{const button=[...document.querySelectorAll('.scope-panel [data-scope]')].find(e=>e.dataset.scope===${JSON.stringify(id)});button.click();})()`,packages,true);await idle();};
 const choose=async artifact=>{
  await idle();await cdp.evaluate(`document.querySelector('[role="tab"][aria-selected="false"]').textContent.includes('Local library')&&document.querySelector('[role="tab"][aria-selected="false"]').click()`,packages,true);
  await cdp.evaluate(`(()=>{const row=[...document.querySelectorAll('.package-table tbody tr')].find(r=>r.dataset.archiveDigests.split(' ').includes(${JSON.stringify(artifact.digest)}));if(!row)throw Error('package row missing');row.click();})()`,packages,true);
  await eventually(()=>cdp.evaluate(`document.querySelector('.package-table tr[aria-selected="true"]')?.dataset.archiveDigests.split(' ').includes(${JSON.stringify(artifact.digest)})`,packages),'package selected');
  await cdp.evaluate(`(()=>{const version=document.querySelector('select[aria-label="Package version"]');if(version){version.value=${JSON.stringify(artifact.digest)};version.dispatchEvent(new Event('change',{bubbles:true}));}})()`,packages,true);
  await eventually(()=>cdp.evaluate(`document.querySelector('select[aria-label="Package version"]')?.value===${JSON.stringify(artifact.digest)}||!document.querySelector('select[aria-label="Package version"]')`,packages),'version selected');
  const label=await cdp.evaluate(`document.querySelector('.details-actions .primary')?.textContent`,packages);await cdp.click(label,packages);await cdp.waitText('Install extension',packages);await cdp.click('Install',packages);
  await idle();await eventually(()=>cdp.evaluate('!document.querySelector("dialog:modal")',packages),'review closed');
 };
 await scope('world:'+world);await choose(v1);let selected=(await call({method:'selections',scopeId:'world:'+world})).value;assert.equal(selected.length,1);const instance=selected[0].instanceId;assert.equal(selected[0].digest,v1.digest);
 await choose(same);selected=(await call({method:'selections',scopeId:'world:'+world})).value;assert.equal(selected[0].digest,same.digest);assert.equal(selected[0].instanceId,instance);
 await choose(v2);selected=(await call({method:'selections',scopeId:'world:'+world})).value;assert.equal(selected[0].digest,v2.digest);assert.equal(selected[0].instanceId,instance);
 assert.equal((await call({method:'selections',scopeId:'host'})).value.some(r=>r.subject==='rintawa.rtwkit.dice'),false);
 checks.push('a stopped World switches exact versions while preserving its instance identity and leaving Home unchanged');
 await writeFile(join(root,'launcher-installed.png'),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
 await cdp.click('Disable',packages);await idle();await eventually(async()=>!(await call({method:'selections',scopeId:'world:'+world})).value[0].enabled,'disabled persisted');
 await cdp.click('Enable',packages);await idle();await eventually(async()=>(await call({method:'selections',scopeId:'world:'+world})).value[0].enabled,'enabled persisted');
 await cdp.click('Remove from scope',packages);await idle();await eventually(async()=>(await call({method:'selections',scopeId:'world:'+world})).value.length===0,'removed persisted');
 inventory=await inventoryPage();assert.equal(inventory.items.filter(r=>r.subject==='rintawa.rtwkit.dice').length,3);assert.deepEqual(await readFile(join(home,'worlds',world,'world.sqlite')),originalWorld);
 checks.push('disable/enable/remove persist independently; scope uninstall preserves all archives and the World database');
 await scope('default-world');await choose(v1);assert.equal((await call({method:'selections',scopeId:'default-world'})).value[0].digest,v1.digest);assert.equal((await call({method:'selections',scopeId:'world:'+world})).value.length,0);
 checks.push('new-World defaults are independent of existing Worlds');
 // Native lifecycle transitions are deliberate Root operations, not inherited by
 // the manager's isolated child channel.
 await rpc('world-sessions.set-active',{worldId:world,active:true});await eventually(async()=>(await rpc('world-sessions.list-worlds')).find(w=>w.worldId===world).active,'World started');
 const denied=await call({method:'select',scopeId:'world:'+world,digest:v1.digest});assert.ok(denied.error.code.includes('Stop the World'));
 await rpc('world-sessions.set-active',{worldId:world,active:false});await eventually(async()=>!(await rpc('world-sessions.list-worlds')).find(w=>w.worldId===world).active,'World stopped');
 checks.push('backend rejects changing a running World and never implicitly stops or starts it');
 await imported(join(artifacts,'TextRoleplay.rtw'));
 const templates=await rpc('world-sessions.list-templates');assert.ok(templates.some(t=>t.name==='Text Roleplay'));
 await cdp.setInput('input[aria-label="Search packages"]','Text Roleplay',packages);
 await cdp.evaluate("document.querySelector('.package-table tbody tr').click()",packages,true);
 await cdp.click('Create World…',packages);await cdp.waitText('Create a world');await eventually(()=>cdp.evaluate('!!document.querySelector(".template-approval input")'),'template permissions rendered');
 assert.equal(await cdp.evaluate('document.querySelector("#world-template").value'),templates.find(t=>t.name==='Text Roleplay').digest);
 assert.equal(await cdp.evaluate('document.querySelector(".template-approval input").checked'),false);
 await cdp.click('Cancel');assert.equal((await rpc('world-sessions.list-worlds')).length,1);
 await cdp.setInput('input[aria-label="Search packages"]','',packages);
 checks.push('a template opens the Root chooser with its exact identity; no World or permissions are created without user submit');
 // A damaged unselected file is a per-row catalog error and does not take away
 // the independent upload capability. Existing strict execution remains intact.
 await writeFile(join(home,'artifacts','sha256','unexpected-file'),'bad');
 await writeFile(join(home,'artifacts','sha256','f'.repeat(64)+'.rtw'),'broken archive');
 await cdp.click('Refresh',packages);await eventually(()=>cdp.evaluate('!document.querySelector("input[type=file]").disabled',packages),'catalog with damaged entries');
 const damaged=await inventoryPage();assert.equal(damaged.items.filter(row=>row.error).length,2);assert.ok(damaged.items.some(row=>row.subject==='rintawa.rtwkit.dice'));
 checks.push('malformed filename and unreadable archive remain error rows; valid entries and import remain available');
 await writeFile(join(root,'invalid.rtw'),'not a package');await upload(join(root,'invalid.rtw'),packages);await cdp.waitText('Import was not confirmed.',packages);assert.equal((await inventoryPage()).items.filter(r=>r.subject==='rintawa.rtwkit.dice').length,3);
 for(const width of [1280,400]){await cdp.call('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:false});assert.equal(await cdp.evaluate('document.documentElement.scrollWidth<=innerWidth',packages),true);await writeFile(join(root,`packages-${width}.png`),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));}
 checks.push('invalid archive is rejected without selecting anything; desktop/mobile layout stays within its view');
 await cdp.call('Emulation.setDeviceMetricsOverride',{width:1280,height:900,deviceScaleFactor:1,mobile:false});
 await cdp.click('Worlds');await eventually(async()=>(await cdp.panelContexts('.packages')).length===0,'Host app unmounted');await cdp.click('Packages');const reopened=await panel('.packages');await cdp.waitText('Local library',reopened);
 checks.push('Home app navigation releases the isolated view and reopens it without changing package configuration');
 assert.deepEqual(cdp.errors,[]);assert.deepEqual(cdp.resourceFailures,[]);
 console.log(JSON.stringify({root,checks},null,2));
}finally{
 if(cdp){try{await writeFile(join(root,'body.txt'),redact(await cdp.body()));const frames=await cdp.panelContexts('.packages');for(let i=0;i<frames.length;i++)await writeFile(join(root,'packages-body-'+i+'.txt'),redact(await cdp.body(frames[i])));await writeFile(join(root,'last-page.png'),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));}catch{}}
 await writeFile(join(root,'checks.json'),JSON.stringify({checks,timings,errors:cdp?.errors,resourceFailures:cdp?.resourceFailures},null,2));await writeFile(join(root,'host.log'),redact(hostOutput));await writeFile(join(root,'chromium.log'),redact(chromeOutput));
 if(chrome&&chrome.exitCode===null){chrome.kill('SIGTERM');await Promise.race([new Promise(r=>chrome.once('exit',r)),pause(3000)]);if(chrome.exitCode===null)chrome.kill('SIGKILL');}
 if(host&&host.exitCode===null){host.kill('SIGINT');await Promise.race([new Promise(r=>host.once('exit',r)),pause(10000)]);if(host.exitCode===null)host.kill('SIGKILL');assert.equal(host.exitCode,0);}
 console.log('Evidence:',root);
}
