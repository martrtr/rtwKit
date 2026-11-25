// Real browser/WASM: scoped UI protocols, upfront consent and optional real Ollama.
import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createServer} from 'node:http';
import {CDP,eventually,pause,redact} from './browser-driver.mjs';
const core=process.env.RINTAWA_BINARY,artifacts=process.env.RTWKIT_SMOKE_ARTIFACTS;
if(!core||!artifacts)throw Error('Set RINTAWA_BINARY and RTWKIT_SMOKE_ARTIFACTS');
const root=await mkdtemp(join(tmpdir(),'rtwkit-streaming-')),home=join(root,'home'),checks=[];
const model=process.env.RTWKIT_OLLAMA_MODEL??'fixture-model';let origin=process.env.RTWKIT_OLLAMA_ORIGIN??'http://127.0.0.1:11434',server;
let host,chrome,cdp,hostOutput='',chromeOutput='',active,emitPrefix=true;const requests=[];
const cli=(...args)=>{const r=spawnSync(core,['--home',home,...args],{encoding:'utf8'});assert.equal(r.status,0,redact(r.stderr));return r.stdout;};
const install=(slug,permissions)=>{const output=cli('install',join(artifacts,slug+'.rtw'));const id=/^instance = (.+)$/m.exec(output)[1];for(const [component,grants]of Object.entries(permissions))for(const grant of grants)cli('grant-runtime',id,component,grant);return id;};
const panel=selector=>eventually(async()=>(await cdp.panelContexts(selector))[0],selector,30000);
const rpc=(method,args={})=>cdp.evaluate(`window.__protocolAdmin.rpc(${JSON.stringify(method)},${JSON.stringify(args)})`);
const query=(world,title,point,call)=>cdp.evaluate(`window.__protocolAdmin.query(${[world,title,point,call].map(JSON.stringify).join(',')})`);
const readQuery=async(...args)=>{for(let i=0;;i++){try{return await query(...args);}catch(error){if(i>=9||!/unavailable/.test(error.message))throw error;await pause(100);}}};
async function settings(){await cdp.openSettings();const renderer=await panel('.settings-layout');await cdp.waitText('Ai Provider',renderer);await cdp.click('Ai Provider',renderer);const ai=await panel('.ai-settings');await cdp.waitText('Ai Provider',ai);await eventually(()=>cdp.evaluate('!!document.querySelector(".ai-toolbar")',ai),'AI settings loaded');return {renderer,ai};}
async function approve(){await eventually(()=>cdp.evaluate('[...document.querySelectorAll("dialog:modal button")].some(b=>b.textContent==="Allow selected"&&!b.disabled)'),'consent ready');await cdp.click('Allow selected');await eventually(()=>cdp.evaluate('![...document.querySelectorAll("dialog:modal h2")].some(h=>h.textContent==="Allow access")'),'consent closed');}
async function upload(file,context){await cdp.call('DOM.enable',{},context.sessionId);const input=await cdp.call('Runtime.evaluate',{expression:'document.querySelector("input[type=file]")',contextId:context.id},context.sessionId);const {node}=await cdp.call('DOM.describeNode',{objectId:input.result.objectId},context.sessionId);await cdp.call('DOM.setFileInputFiles',{backendNodeId:node.backendNodeId,files:[file]},context.sessionId);await cdp.call('Runtime.releaseObject',{objectId:input.result.objectId},context.sessionId);}
async function create(name){await cdp.click('New World');await cdp.setInput('#world-name',name);await eventually(()=>cdp.evaluate('!!document.querySelector(".template-approval input")'),'creation access ready');await cdp.evaluate('document.querySelector(".template-requirements summary").click()');const text=await cdp.body();assert.ok(text.includes(origin),'known AI origin is in the common permission list');assert.equal(await cdp.evaluate('!!document.querySelector(".template-access-summary")'),false);assert.ok(text.includes('published-preference-write'));assert.equal(await cdp.evaluate('document.querySelector("dialog button[type=submit]").disabled'),true);await cdp.evaluate('document.querySelector(".template-approval input").click()');await cdp.click('Create world');await eventually(async()=>(await cdp.body()).includes('Stop World'),'World activation',60000);const welcome=await panel('.story-welcome');await cdp.waitText('Import a character',welcome);return welcome;}
try{
 assert.equal(process.env.RTWKIT_OLLAMA_MODEL,undefined,'loopback fixture only');
 server=createServer((request,response)=>{let data='';request.on('data',b=>data+=b);request.on('end',()=>{
  if(request.method==='GET'){response.setHeader('content-type','application/json');response.end(JSON.stringify({data:[{id:model}]}));return;}
  requests.push(JSON.parse(data));active=response;response.writeHead(200,{'content-type':'text/event-stream'});
  const bytes=Buffer.from(`data: ${JSON.stringify({choices:[{index:0,delta:{content:'Streaming При'},finish_reason:null}]})}\n\n`),cut=bytes.indexOf(Buffer.from('При'))+1;
  response.flushHeaders();if(emitPrefix){response.write(bytes.subarray(0,cut));setTimeout(()=>{if(!response.destroyed)response.write(bytes.subarray(cut));},20);}
 });});await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));origin=`http://127.0.0.1:${server.address().port}`;

 install('web-runtime',{runtime:['background-task','loopback-listen']});
 install('web-ui',{shell:['background-task','world-session-read','world-session-write','runtime-policy-write'],home:['background-task','world-session-read']});
 install('settings-ui',{settings:['background-task'],appearance:['background-task']});
 const baselineAi=install('ai-provider',{backend:['background-task'],settings:['background-task']});
 cli('package-install',join(artifacts,'TextRoleplay.rtw'));
 let output='';host=spawn(core,['--home',home,'run'],{env:{...process.env,RUST_LOG:'info'},stdio:['ignore','pipe','pipe']});for(const stream of[host.stdout,host.stderr])stream.on('data',b=>{output+=b;hostOutput+=b;});
 const launch=await eventually(()=>{if(host.exitCode!==null)throw Error('Host exited');return /http:\/\/127\.0\.0\.1:\d+\/shell\/[^\s\x1b]+/.exec(output)?.[0];},'Host launch',30000);
 chrome=spawn('/usr/bin/chromium',['--headless=new','--no-sandbox','--disable-dev-shm-usage','--disable-background-networking','--disable-component-update','--no-first-run','--no-default-browser-check','--remote-debugging-port=0',`--user-data-dir=${join(root,'chromium')}`,'about:blank'],{stdio:['ignore','ignore','pipe']});chrome.stderr.on('data',b=>chromeOutput+=b);
 const port=await eventually(async()=>{try{return(await readFile(join(root,'chromium/DevToolsActivePort'),'utf8')).split('\n')[0];}catch{return null;}},'Chrome debugger');const targets=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();cdp=await CDP.connect(targets.find(t=>t.type==='page').webSocketDebuggerUrl);
 await cdp.call('Target.setAutoAttach',{autoAttach:true,waitForDebuggerOnStart:false,flatten:true});for(const method of['Runtime.enable','Page.enable','Network.enable','Log.enable'])await cdp.call(method);await cdp.call('Emulation.setDeviceMetricsOverride',{width:1280,height:900,deviceScaleFactor:1,mobile:false});cdp.rootFrame=(await cdp.call('Page.navigate',{url:launch})).frameId;await cdp.waitShell();
 const homeFrame=await panel('.home-content');await cdp.waitText('Your worlds',homeFrame);assert.equal(await cdp.evaluate('!!document.querySelector(".home-content")'),false);checks.push('Home content belongs to the separately selected ordinary Home provider');
 await cdp.evaluate(`(async()=>{const ws=new WebSocket('ws://'+location.host+'/bridge');let root,n=0;const pending=new Map();await new Promise((resolve,reject)=>{ws.onopen=()=>ws.send(JSON.stringify({v:1,type:'auth',token:sessionStorage.getItem('rintawa.session')}));ws.onerror=reject;ws.onmessage=e=>{const v=JSON.parse(e.data);if(v.type==='ready'){root=v.channel;resolve();}else{const p=pending.get(v.id);if(p){pending.delete(v.id);clearTimeout(p.timer);v.ok?p.resolve(v.value):p.reject(Error(v.error.code));}}};});const rpc=(channel,method,args={})=>new Promise((resolve,reject)=>{const id=String(++n),timer=setTimeout(()=>{pending.delete(id);reject(Error('rpc timeout'));},5000);pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({v:1,id,channel,method,args}));});window.__protocolAdmin={rpc:(method,args)=>rpc(root,method,args),channel:(channel,method,args)=>rpc(channel,method,args),query:async(world,title,point,call)=>{const views=await rpc(root,'ui.list'),view=views.filter(v=>v.worldId===world&&v.title===title);if(view.length!==1)throw Error('ambiguous view');const binding=await rpc(root,'ui.mount',{id:view[0].id});try{const handle=await rpc(binding.channel,'transport.begin-query',{point,payload:btoa(String.fromCharCode(...new TextEncoder().encode(JSON.stringify(call))))});for(let i=0;i<200;i++){const replies=await rpc(binding.channel,'transport.poll-query',{handle});if(replies!==null){if(replies.length===0)throw Error('unavailable');if(replies.length!==1)throw Error('ambiguous reply');return JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(replies[0]),c=>c.charCodeAt(0))));}await new Promise(r=>setTimeout(r,20));}throw Error('query timeout');}finally{await rpc(root,'ui.unmount',{channel:binding.channel});}}};})()`);
 let {renderer,ai}=await settings();assert.equal(await cdp.evaluate('!!document.querySelector(".settings-navigation")'),false);assert.equal(await cdp.evaluate('window.parent===window.top',ai),true);checks.push('unopened AI and Appearance contributions are discovered over sourced Zenoh; isolated child is owned by Root');
 await cdp.click('Add provider',ai);await cdp.click('Ollama',ai);await cdp.setInput('#ai-label','Local test Ollama',ai);await cdp.setInput('#ai-url',origin+'/v1',ai);await cdp.setInput('#ai-model',model,ai);await cdp.click('Save provider',ai);await approve();await cdp.waitText('Online',ai);await cdp.click('Close dialog');
 const welcome=await create('Protocol A');const worlds=await rpc('world-sessions.list-worlds');const a=worlds.find(w=>w.title==='Protocol A');const views=await rpc('ui.list'),aiView=views.find(v=>v.worldId===a.worldId&&v.title==='Ai Provider');
 const before=await rpc('runtime-policy.live-component',{scopeId:`world:${a.worldId}`,instanceId:aiView.restoreKey.instanceId,componentId:'backend'});assert.ok(before.granted.includes('http-request'));assert.ok(before.granted.includes('published-preference-read'));assert.deepEqual(before.httpOrigins,[origin]);checks.push('one creation consent saves AI runtime rights and known local origin under the fresh World identity before activation');
 const card=join(root,'synthetic.json');await writeFile(card,JSON.stringify({spec:'chara_card_v2',spec_version:'2.0',data:{name:'Ada',description:'A fictional librarian in a local software test.',personality:'Friendly.',scenario:'A quiet library.',first_mes:'Welcome to the test library.',mes_example:'',creator_notes:'',system_prompt:'Stay in character. Reply in one short sentence.',post_history_instructions:'',alternate_greetings:[],tags:[],creator:'test',character_version:'1',extensions:{}}}));await upload(card,welcome);let chat=await panel('.chat');await cdp.waitText('Welcome to the test library.',chat);
 const inherited=await readQuery(a.worldId,'Chat','rintawa/inference/v1',{method:'describe'});assert.equal(inherited.value.ready,true);checks.push('fresh World uses the global primary without opening World AI Settings or selecting a preset');

 const conversation=(await readQuery(a.worldId,'Chat','rintawa/chat/v1',{method:'list'})).value.items[0].id;
 const recent=async()=>(await readQuery(a.worldId,'Chat','rintawa/chat/v1',{method:'recent',conversationId:conversation})).value;
 const send=async text=>{await eventually(()=>cdp.evaluate('!document.querySelector("#message").disabled',chat),'composer ready');const count=requests.length;await cdp.setInput('#message',text,chat);await cdp.click('Send',chat);await eventually(()=>requests.length===count+1,'new HTTP request admitted');if(emitPrefix)await eventually(()=>cdp.evaluate('document.querySelector(".reply-preview.streaming p")?.textContent==="Streaming При"',chat),'new operation prefix');else await eventually(()=>cdp.evaluate('!!document.querySelector(".reply-typing")',chat),'one typing indicator');};
 const shot=async name=>writeFile(join(root,name+'.png'),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
 const idle=(timeout=30000)=>eventually(()=>cdp.evaluate('!document.querySelector("#message").disabled',chat),'generation terminal',timeout);
 const chunk=(text,finish=null)=>`data: ${JSON.stringify({choices:[{index:0,delta:{content:text},finish_reason:finish}]})}\n\n`;
 await send('Stream a reply');
 assert.equal((await recent()).items.filter(r=>r.message.role==='assistant').length,1,'only greeting committed');
 await pause(300);assert.equal(await cdp.evaluate('document.querySelector(".reply-preview p").textContent',chat),'Streaming При');
 assert.equal(await cdp.evaluate('document.querySelector(".reply-preview").getAttribute("aria-busy")',chat),'true');assert.equal(await cdp.evaluate('/Generating…|Working…|not saved/.test(document.querySelector(".chat").innerText)',chat),false);
 await cdp.evaluate('(()=>{const v=document.querySelector(".transcript");v.style.maxHeight="80px";v.scrollTop=0;})()',chat);
 active.write(chunk('вет'));await cdp.waitText('Streaming Привет',chat);
 assert.equal(await cdp.evaluate('document.querySelector(".transcript").scrollTop',chat),0);
 await cdp.evaluate('(()=>{const v=document.querySelector(".transcript");v.style.maxHeight="";v.scrollTop=v.scrollHeight;})()',chat);await shot('prefix');
 active.end(chunk(' 🌍.','stop')+'data: [DONE]\n\n');await idle();
 await eventually(()=>cdp.evaluate('!document.querySelector(".reply-preview")',chat),'preview replaced');
 let page=await recent();assert.equal(page.items.filter(r=>r.message.text==='Streaming Привет 🌍.').length,1);assert.equal(await cdp.evaluate('[...document.querySelectorAll("article.message p")].filter(p=>p.textContent==="Streaming Привет 🌍.").length',chat),1);
 checks.push('fragmented UTF-8 prefix is visible before completion/COMMIT; repeated snapshots append without duplication; reader scroll is preserved; one final canonical answer');
 const originalTitle=page.conversation.title, originalHistory=JSON.stringify(page);
 const beforeRegenerate=requests.length;
 await cdp.evaluate('document.querySelector("article.message:last-of-type details summary").click()',chat);await cdp.click('Regenerate',chat);
 await eventually(()=>requests.length===beforeRegenerate+1,'regeneration request on new branch');
 await cdp.waitText('Streaming При',chat);
 const branchList=(await readQuery(a.worldId,'Chat','rintawa/chat/v1',{method:'list'})).value.items;
 const branch=branchList.find(row=>row.id!==conversation&&row.conversation.title.endsWith(' · Regenerated'));assert.ok(branch);
 const readBranch=async()=>(await readQuery(a.worldId,'Chat','rintawa/chat/v1',{method:'recent',conversationId:branch.id})).value;
 assert.deepEqual((await readBranch()).items.map(row=>row.message.text),page.items.slice(0,-1).map(row=>row.message.text));
 assert.equal(requests.at(-1).messages.some(message=>message.content==='Streaming Привет 🌍.'),false,'old answer omitted from regeneration prompt');
 const regenerateResponse=active;await cdp.click('Stop',chat);await idle();
 await eventually(async()=>(await readBranch()).items.at(-1)?.message.role==='assistant','regenerated prefix committed only in branch');
 assert.equal((await readBranch()).items.at(-1).message.text,'Streaming При');
 assert.equal(JSON.stringify(await recent()),originalHistory,'original unchanged by regeneration and Stop');
 if(!regenerateResponse.destroyed)regenerateResponse.end(chunk(' TOO LATE','stop')+'data: [DONE]\n\n');
 await cdp.selectConversation(originalTitle,chat);await cdp.waitText('Streaming Привет 🌍.',chat);
 checks.push('Regenerate publishes an independent chat before inference; prompt excludes old answer; Stop saves only into that branch and original remains exact');
 // An edited user prefix can request a reply explicitly without adding another user turn.
 await cdp.evaluate('(()=>{const menu=document.querySelectorAll("article.message")[1].querySelector("details");if(!menu.open)menu.querySelector("summary").click();[...menu.querySelectorAll("button")].find(button=>button.textContent==="Edit").click();})()',chat);
 await cdp.setInput('#replacement-message','Revised input for a new continuation',chat);await cdp.click('Save as new chat',chat);
 await eventually(()=>cdp.evaluate('!document.querySelector("dialog:modal")&&document.querySelectorAll("article.message").length===2',chat),'edited user prefix');
 const editedBranch=(await readQuery(a.worldId,'Chat','rintawa/chat/v1',{method:'list'})).value.items.find(row=>row.id!==conversation&&row.conversation.title.endsWith(' · Edited'));assert.ok(editedBranch);
 const editedRequests=requests.length;
 await cdp.evaluate('document.querySelector("article.message:last-of-type details summary").click()',chat);await cdp.click('Generate reply',chat);
 await eventually(()=>requests.length===editedRequests+1,'explicit reply to edited user');
 assert.equal(requests.at(-1).messages.at(-1).content,'Revised input for a new continuation');
 await cdp.waitText('Streaming При',chat);const editedResponse=active;await cdp.click('Stop',chat);await idle();
 const editedPage=(await readQuery(a.worldId,'Chat','rintawa/chat/v1',{method:'recent',conversationId:editedBranch.id})).value;
 assert.equal(editedPage.items.length,3);assert.equal(editedPage.items.at(-1).message.text,'Streaming При');
 assert.equal(JSON.stringify(await recent()),originalHistory);
 if(!editedResponse.destroyed)editedResponse.end(chunk(' TOO LATE','stop')+'data: [DONE]\n\n');
 await cdp.selectConversation(originalTitle,chat);await cdp.waitText('Streaming Привет 🌍.',chat);
 checks.push('editing a user starts an independent prefix; Generate reply uses the replacement without an extra user turn and preserves the original');
 await send('Interrupt a reply');await cdp.evaluate('(()=>{const v=document.querySelector(".transcript");v.style.maxHeight="80px";v.scrollTop=0;})()',chat);active.end();await idle();
 await eventually(()=>cdp.evaluate('!document.querySelector(".reply-preview")',chat),'interrupted text becomes a canonical message');
 page=await recent();assert.equal(page.items.filter(r=>r.message.role==='assistant').length,3);assert.equal(page.items.at(-1).message.text,'Streaming При');assert.equal(page.items.at(-1).message.role,'assistant');assert.equal(page.items.at(-1).message.revision,'1');
 assert.equal(await cdp.evaluate('document.querySelector(".transcript").scrollTop',chat),0);
 await cdp.evaluate('document.querySelector(".transcript").style.maxHeight=""',chat);await shot('retained');checks.push('a broken stream retains its validated prefix as one ordinary versioned assistant message without failure captions');
 let retainedCount=3;
 await send('Save this prefix when I press Stop');const stoppedResponse=active;
 await cdp.click('Stop',chat);await idle();
 await eventually(()=>cdp.evaluate('!document.querySelector(".reply-preview")',chat),'stopped prefix canonical');
 page=await recent();assert.equal(page.items.at(-1).message.text,'Streaming При');
 assert.equal(page.items.filter(r=>r.message.role==='assistant').length,++retainedCount);
 assert.equal(await cdp.evaluate('!!document.querySelector(".reply-error")',chat),false);
 // No heartbeat/data wakes the silent upstream before the save is observed.
 // Current native HTTP cancellation retires a blocking read on data or timeout.
 if(!stoppedResponse.destroyed)stoppedResponse.end(chunk(' MUST NOT BE SAVED','stop')+'data: [DONE]\n\n');
 await pause(300);page=await recent();assert.equal(page.items.at(-1).message.text,'Streaming При');
 assert.equal(page.items.filter(r=>r.message.role==='assistant').length,retainedCount);
 checks.push('explicit Stop saves exactly the Narrator prefix before silent upstream is woken; later provider output cannot grow or duplicate the saved answer');
 // Exercise owner/idempotency and stale-append guards through a retained
 // ordinary Chat UI channel; no production testing hooks are introduced.
 await cdp.evaluate(`(async()=>{const admin=window.__protocolAdmin,view=(await admin.rpc('ui.list')).find(v=>v.worldId===${JSON.stringify(a.worldId)}&&v.title==='Chat');const binding=await admin.rpc('ui.mount',{id:view.id});window.__stopProbe={channel:binding.channel,call:async(point,call)=>{const handle=await admin.channel(binding.channel,'transport.begin-query',{point,payload:btoa(String.fromCharCode(...new TextEncoder().encode(JSON.stringify(call))))});try{for(let i=0;i<300;i++){const rows=await admin.channel(binding.channel,'transport.poll-query',{handle});if(rows!==null){if(rows.length!==1)throw Error(rows.length?'ambiguous probe response':'unavailable');return JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(rows[0]),c=>c.charCodeAt(0))));}await new Promise(r=>setTimeout(r,20));}throw Error('probe timeout');}finally{await admin.channel(binding.channel,'transport.close-query',{handle}).catch(error=>{if(!error.message.includes('invalid-handle'))throw error;});}}};})()`);
 const probe=async(point,call)=>{for(let attempt=0;;attempt++){try{return await cdp.evaluate(`window.__stopProbe.call(${JSON.stringify(point)},${JSON.stringify(call)})`);}catch(error){if(attempt>=9||!['status','release','allocate'].includes(call.method)||!/unavailable/.test(error.message))throw error;await pause(100);}}};
 const chatPoint='rintawa/chat/v1',narratorPoint='rintawa/narrator/v1';
 const waitProbe=async(point,op)=>eventually(async()=>{const value=(await probe(point,{method:'status',operation:op.operation})).value;return value.status==='pending'?null:value;},'probe terminal');
 const rawSend=async text=>{const ids=(await probe(chatPoint,{method:'allocate',count:2})).value;const op=(await probe(chatPoint,{method:'begin-send',conversationId:conversation,messageId:ids[0],entryId:ids[1],text})).value;assert.equal((await waitProbe(chatPoint,op)).status,'committed');await probe(chatPoint,{method:'release',operation:op.operation});return ids[0];};
 const trigger=await rawSend('Stop must respect the original captured history');
 const answerIds=(await probe(chatPoint,{method:'allocate',count:2})).value;
 const generation=(await probe(narratorPoint,{method:'begin-generate',conversationId:conversation,userMessageId:trigger,messageId:answerIds[0],entryId:answerIds[1]})).value;
 await eventually(async()=>(await probe(narratorPoint,{method:'status',operation:generation.operation})).value.text==='Streaming При','probe prefix');
 const staleResponse=active;
 const foreign=await readQuery(a.worldId,'Ai Provider',narratorPoint,{method:'stop',operation:generation.operation});assert.equal(foreign.error.code,'invalid-operation');
 await rawSend('A newer user message invalidates the old reply');
 assert.equal((await probe(narratorPoint,{method:'stop',operation:generation.operation})).ok,true);
 assert.equal((await probe(narratorPoint,{method:'stop',operation:generation.operation})).ok,true);
 const stale=await waitProbe(narratorPoint,generation);assert.equal(stale.status,'failed');assert.ok(stale.reason.includes('stale-context'));
 await probe(narratorPoint,{method:'release',operation:generation.operation});
 if(!staleResponse.destroyed)staleResponse.end();
 page=await recent();assert.equal(page.items.some(row=>row.messageId===answerIds[0]),false);
 assert.equal(page.items.filter(r=>r.message.role==='assistant').length,retainedCount);
 await rpc('ui.unmount',{channel:await cdp.evaluate('window.__stopProbe.channel')});
 checks.push('foreign caller cannot stop work; repeated owner Stop is safe; a newer Chat commit rejects stale retained text without an extra message');
 emitPrefix=false;await send('Stop before the first character');const emptyStoppedResponse=active;
 await cdp.click('Stop',chat);await idle();
 assert.equal(await cdp.evaluate('!!document.querySelector(".reply-preview")',chat),false);
 assert.equal(await cdp.evaluate('!!document.querySelector(".reply-error")',chat),false);
 page=await recent();assert.equal(page.items.filter(r=>r.message.role==='assistant').length,retainedCount);
 if(!emptyStoppedResponse.destroyed)emptyStoppedResponse.end(chunk('TOO LATE','stop')+'data: [DONE]\n\n');
 checks.push('explicit Stop before any text creates no assistant message and no interrupted-response error');emitPrefix=true;
 if(process.env.RTWKIT_TEST_TIMEOUT==='1'){
  await send('Keep the timed-out reply');await idle(110000);
  await eventually(()=>cdp.evaluate('!document.querySelector(".reply-preview")',chat),'timed-out reply committed');
  page=await recent();assert.equal(page.items.at(-1).message.text,'Streaming При');assert.equal(page.items.at(-1).message.role,'assistant');assert.equal(page.items.filter(r=>r.message.role==='assistant').length,++retainedCount);assert.equal(await cdp.evaluate('!!document.querySelector(".reply-error")',chat),false);await shot('timeout-retained');
  checks.push('actual HTTP deadline retains the streamed prefix as an ordinary message without duplicate timeout/status notices');
 }
 emitPrefix=false;await send('Stop this generation');
 assert.equal(await cdp.evaluate('document.querySelectorAll(".reply-typing").length',chat),1);assert.equal(await cdp.evaluate('document.querySelector("#message").placeholder',chat),'');assert.equal(await cdp.evaluate('/Generating…|Working…/.test(document.querySelector(".chat").innerText)',chat),false);await shot('waiting');
 active.end();await idle();await cdp.waitText('Response interrupted.',chat);assert.equal(await cdp.evaluate('!!document.querySelector(".reply-preview")',chat),false);
 for(const width of [1280,640,400]){await cdp.call('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:false});await eventually(()=>cdp.evaluate('(()=>{const n=document.querySelector(".thread-notices"),c=document.querySelector(".composer");return Math.abs(n.getBoundingClientRect().left+parseFloat(getComputedStyle(n).paddingLeft)-c.getBoundingClientRect().left)<1;})()',chat),'error aligned to composer');}
 assert.equal(await cdp.evaluate('document.querySelectorAll(".reply-error [role=alert]").length',chat),1);assert.equal(await cdp.evaluate('document.querySelectorAll(".thread-actions button").length',chat),1);await shot('empty-error');
 checks.push('one typing indicator; no busy placeholder/labels; an empty failed reply shows one short aligned error and retry at desktop/mobile widths');
 await cdp.call('Emulation.setDeviceMetricsOverride',{width:1280,height:900,deviceScaleFactor:1,mobile:false});emitPrefix=true;const prior=requests.length;await cdp.click('Retry reply',chat);await eventually(()=>requests.length===prior+1,'explicit retry only');await cdp.waitText('Streaming При',chat);const held=active;await cdp.click('Workspace controls');await cdp.click('Close active view');await eventually(async()=>(await cdp.panelContexts('.chat')).length===0,'frame retired');await cdp.click('Stop World');
 await eventually(async()=>(await rpc('world-sessions.list-worlds')).every(w=>!w.active&&w.pendingActive===null),'World stopped');
 // Existing Core worker observes cancellation between blocking reads. Unblock its
 // silent upstream read; this test does not claim immediate socket cancellation.
 if(!held.destroyed)held.write(': teardown probe\n\n');
 await eventually(()=>held.destroyed,'cancelled worker closes after its read unblocks');const count=requests.length;
 await cdp.click('Start World');await eventually(async()=>(await cdp.body()).includes('Stop World'),'World activation',60000);chat=await panel('.chat');await cdp.waitText('Stop this generation',chat);await pause(500);
 page=await recent();assert.equal(page.items.filter(r=>r.message.role==='assistant').length,retainedCount);assert.equal(page.items.some(r=>r.message.text==='Streaming При'&&r.message.role==='assistant'),true);assert.equal(requests.length,count);assert.equal(await cdp.evaluate('!!document.querySelector(".reply-preview")',chat),false);await shot('restart');
 checks.push('frame retirement fences provisional UI; World stop closes HTTP; restart preserves canonical history without partial reply or automatic generation replay');
 assert.deepEqual(cdp.errors,[]);assert.deepEqual(cdp.resourceFailures,[]);console.log(JSON.stringify({root,checks},null,2));
}finally{
 if(cdp){try{await writeFile(join(root,'last-page.png'),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));await writeFile(join(root,'body.txt'),redact(await cdp.body()));}catch{}}
 await writeFile(join(root,'checks.json'),JSON.stringify({checks,errors:cdp?.errors,resourceFailures:cdp?.resourceFailures,requests},null,2));await writeFile(join(root,'host.log'),redact(hostOutput));await writeFile(join(root,'chromium.log'),redact(chromeOutput));
 if(chrome&&chrome.exitCode===null){chrome.kill('SIGTERM');await Promise.race([new Promise(r=>chrome.once('exit',r)),pause(3000)]);if(chrome.exitCode===null)chrome.kill('SIGKILL');}
 if(host&&host.exitCode===null){host.kill('SIGINT');await Promise.race([new Promise(r=>host.once('exit',r)),pause(10000)]);if(host.exitCode===null)host.kill('SIGKILL');assert.equal(host.exitCode,0);}
 if(server){server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}console.log('Evidence:',root);
}
