// Deterministic loopback-only acceptance, using the real Chat UI and WASM owners.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { deflateSync } from 'node:zlib';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { eventually, pause } from './browser-driver.mjs';
export function characterPng(card) {
  const crc32 = bytes => { let crc=0xffffffff; for(const byte of bytes){crc^=byte;for(let i=0;i<8;i++)crc=(crc>>>1)^(0xedb88320&-(crc&1));} return (crc^0xffffffff)>>>0; };
  const chunk = (type,data) => {const name=Buffer.from(type),size=Buffer.alloc(4),crc=Buffer.alloc(4);size.writeUInt32BE(data.length);crc.writeUInt32BE(crc32(Buffer.concat([name,data])));return Buffer.concat([size,name,data,crc]);};
  const side=Number(process.env.RTWKIT_AI_PNG_SIDE ?? 96);
  const ihdr=Buffer.alloc(13);ihdr.writeUInt32BE(side,0);ihdr.writeUInt32BE(side,4);ihdr[8]=8;ihdr[9]=6;
  const pixels=Buffer.alloc(side*(side*4+1));let random=12345;
  for(let y=0;y<side;y++)for(let x=0;x<side*4;x++){random^=random<<13;random^=random>>>17;random^=random<<5;pixels[y*(side*4+1)+1+x]=x%4===3?255:random&255;}
  const metadata=Buffer.from('ccv3\0'+Buffer.from(JSON.stringify(card)).toString('base64'));
  return Buffer.concat([Buffer.from('89504e470d0a1a0a','hex'),chunk('IHDR',ihdr),chunk('tEXt',metadata),chunk('IDAT',deflateSync(pixels)),chunk('IEND',Buffer.alloc(0))]);
}
export async function runAiChecks({ cdp, cli, query, worlds, panel, first, second, home, root, checks, retiredViews }) {
  let mode = 'streaming'; const requests = []; const pending = new Set(); const live = new Set();
  const text = 'Local deterministic reply: Привет.';
  const emit = (response, content = text) => {
    if (response.destroyed) return;
    response.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: { content }, finish_reason: 'stop' }] })}\n\n`);
    response.end('data: [DONE]\n\n');
  };
  const server = createServer((request, response) => {
    let body = ''; request.on('data', bytes => { body += bytes; if (Buffer.byteLength(body) > 64 * 1024) request.destroy(); });
    live.add(response); response.on('close', () => { live.delete(response); pending.delete(response); });
    request.on('end', () => {
      if (request.method === 'GET') { assert.equal(request.url, '/v1/models'); response.writeHead(200, {'content-type':'application/json'}); response.end(JSON.stringify({data:[{id:'local-fixture'},{id:'updated-fixture'}]})); return; }
      const call = JSON.parse(body); requests.push({ path: request.url, body: call, mode });
      assert.equal(request.method, 'POST'); assert.equal(request.url, '/v1/chat/completions'); assert.equal(call.stream, true);
      assert.equal(request.headers.authorization, undefined);
      if (mode === 'http-error') { response.writeHead(500); response.end('private upstream body must not be exposed'); return; }
      response.writeHead(200, { 'content-type': 'text/event-stream' }); response.flushHeaders();
      if ((mode === 'delayed' || mode === 'timeout')) { if(mode==='timeout')response.write(`data: ${JSON.stringify({choices:[{index:0,delta:{content:'Timed out reply prefix'},finish_reason:null}]})}\n\n`);pending.add(response); const heartbeat = setInterval(() => { if (!response.destroyed) response.write(': keepalive\n\n'); }, 100); response.once('close', () => clearInterval(heartbeat)); return; }
      if (mode === 'streaming' || mode === 'incomplete') {
        pending.add(response);
        const content = mode === 'streaming' ? 'Local deterministic reply: ' : 'Interrupted reply prefix';
        response.write(`data: ${JSON.stringify({choices:[{index:0,delta:{content},finish_reason:null}]})}\n\n`);
        return;
      }
      emit(response, mode === 'oversize' ? 'x'.repeat(16 * 1024 + 1) : text);
    });
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const origin = `http://127.0.0.1:${server.address().port}`;
  const cleanup = async () => { for (const response of live) response.destroy(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); };
  try {
    const markRetired = async world => { for (const view of await cdp.evaluate("window.__storyAdmin.rpc('ui.list')")) if (view.worldId === world.worldId) retiredViews.add(view.id); };
    const stop = async world => { await markRetired(world); await cdp.click('Stop World'); await eventually(async () => { const row = (await worlds()).find(w => w.worldId === world.worldId); return !row.active && row.pendingActive === null; }, 'AI World drains'); };
    const openSettings = async (world) => {
      if (world) await cdp.openWorld(world.title); else await cdp.click('Home');
      await cdp.openSettingsSection('Ai Provider');
      const frame = await panel('.ai-settings'); await eventually(()=>cdp.evaluate('!!document.querySelector(".ai-toolbar") || !!document.querySelector(".ai-connection")',frame),'AI settings ready');
      return frame;
    };
    const consentReady = () => eventually(()=>cdp.evaluate('[...document.querySelectorAll("dialog:modal button")].some(b=>b.textContent==="Allow selected"&&!b.disabled)'),'permission snapshot ready');
    const consentClosed = () => eventually(()=>cdp.evaluate('![...document.querySelectorAll("dialog:modal h2")].some(h=>h.textContent==="Allow access")'),'permission dialog closed');
    const closeSettings = async () => { await consentClosed(); await cdp.click('Close dialog'); };
    const saved = async frame => eventually(()=>cdp.evaluate('!document.querySelector("dialog:modal #ai-label")',frame),'provider saved');
    const policy = (world,instance) => cdp.evaluate(`window.__storyAdmin.rpc('runtime-policy.live-component',{scopeId:${JSON.stringify(world ? `world:${world.worldId}` : 'host')},instanceId:${JSON.stringify(instance)},componentId:'backend'})`);
    const instanceOf = async world => { const profile=await readFile(join(home,'worlds',world.worldId,'composition.toml'),'utf8');return /instance-id = "([^"]+)"/.exec(profile.split('[[activations]]').find(v=>v.includes('subject = "rintawa.rtwkit.ai-provider"')))[1]; };
    const describe = async (world, component = 'Ai Provider') => (await query(world?.worldId ?? null,component,'rintawa/inference/v1',{method:'describe'})).value;
    const clickConnection = (frame, label, action) => eventually(() => cdp.evaluate(`(() => {
      const card = [...document.querySelectorAll('.ai-connection')].find(card => card.querySelector('h3')?.textContent === ${JSON.stringify(label)});
      const button = [...(card?.querySelectorAll('button') ?? [])].find(button => !button.disabled && button.textContent.trim() === ${JSON.stringify(action)});
      if (!button) return false;
      button.click();
      return true;
    })()`,frame,true), `${action} for ${label}`);
    const waitPrimary = (frame, label) => eventually(() => cdp.evaluate(
      `document.querySelector('.ai-connection:has(.ai-selected) h3')?.textContent === ${JSON.stringify(label)}`,frame),
    `${label} is marked primary`);

    let settings = await openSettings(null);
    await cdp.click('Add provider',settings);await cdp.click('Ollama',settings);
    assert.equal(await cdp.evaluate('document.querySelector("#ai-url").value',settings),'http://127.0.0.1:11434/v1');
    assert.equal(await cdp.evaluate('document.querySelector("#ai-auth").value === "bearer"',settings),false);
    await cdp.setInput('#ai-url',origin+'/v1 ',settings);await cdp.setInput('#ai-model','local-fixture',settings);await cdp.click('Save provider',settings);await saved(settings);await consentReady();await cdp.click('Cancel');await consentClosed();
    const initialCatalog = await describe(null);
    assert.equal(initialCatalog.connections.length,1);
    const localConnection = initialCatalog.connections[0];
    assert.equal(initialCatalog.defaultConnectionId,localConnection.id,'first Home connection becomes primary');
    await waitPrimary(settings,'Ollama');
    assert.equal(requests.length,0,'setting up the primary sends no prompts');
    await closeSettings();
    await cdp.openWorld(first.title);const originalChat=await panel('.chat');await cdp.waitText('Saved from the template workflow',originalChat);await eventually(()=>cdp.evaluate('!!document.querySelector("#message")&&!document.querySelector("#message").disabled',originalChat),'A composer ready before consent');await cdp.setInput('#message','Unsent permission draft',originalChat);
    const instance=await instanceOf(first);const beforePolicy=await policy(first,instance);const beforeActivation=beforePolicy.target;
    const bInstance=await instanceOf(second);const bBefore=await policy(second,bInstance);
    assert.ok(beforePolicy.granted.includes('published-preference-write'));
    assert.ok(bBefore.granted.includes('published-preference-write'));
    assert.deepEqual(bBefore.httpOrigins,[]);assert.deepEqual(bBefore.secretRead,[]);
    const beforeActiveCalls=cdp.worldCommands.filter(c=>c.method==='world-sessions.set-active').length;
    settings = await openSettings(first);
    assert.deepEqual((await describe(first)).connections,initialCatalog.connections);
    assert.equal((await describe(first)).defaultConnectionId,localConnection.id);
    const settingsFrame=await cdp.evaluate('location.href',settings);
    await cdp.evaluate(`window.__storyAdmin.rpc('runtime-policy.apply-live-change',{change:{target:${JSON.stringify(beforeActivation)},delta:{revokeRuntime:['published-preference-read']}}})`);
    await cdp.waitText('Allow shared providers…',settings);
    assert.deepEqual(await describe(first,'Chat'),{ready:false,reason:'shared-connections-permission-required'});
    assert.deepEqual(await describe(second,'Chat'),{ready:true,reason:null});
    await cdp.click('Allow shared providers…',settings);await consentReady();await cdp.click('Allow selected');await consentClosed();await waitPrimary(settings,'Ollama');
    assert.deepEqual((await policy(first,instance)).target,beforeActivation,'shared catalog recovery preserves activation');
    await clickConnection(settings,'Ollama','Allow access…');await consentReady();await cdp.click('Cancel');await consentClosed();
    assert.deepEqual((await policy(first,instance)).httpOrigins,beforePolicy.httpOrigins,'cancelling access preserves current grants');
    await clickConnection(settings,'Ollama','Allow access…');await consentReady();
    assert.ok((await cdp.body()).includes('Story A'));assert.ok((await cdp.body()).includes(origin));
    await cdp.click('Allow selected');await consentClosed();await cdp.waitText('Online',settings);
    assert.deepEqual((await policy(first,instance)).target,beforeActivation,'permission update preserves exact activation');
    assert.deepEqual((await policy(first,instance)).httpOrigins,[origin]);
    assert.deepEqual((await policy(second,bInstance)).httpOrigins,bBefore.httpOrigins,'A consent grants no B origin access');
    assert.equal(await cdp.evaluate('location.href',settings),settingsFrame);
    assert.equal(await cdp.evaluate('document.querySelector("#message").value',originalChat),'Unsent permission draft');
    assert.equal(cdp.worldCommands.filter(c=>c.method==='world-sessions.set-active').length,beforeActiveCalls,'consent never restarts a World');
    await closeSettings();let chat=await panel('.chat');await cdp.waitText('Saved from the template workflow',chat);await cdp.waitText('Hello You, I am Story Alice.',chat);
    const profile=await readFile(join(home,'worlds',first.worldId,'composition.toml'),'utf8');
    assert.ok(profile.includes('[[http_loopback_origins]]'));

    assert.equal(requests.length,0,'automatic primary and explicit origin approval do not generate');
    const conversation = (await query(first.worldId, 'Chat', 'rintawa/chat/v1', { method: 'list' })).value.items[0].id;
    const recent = async () => (await query(first.worldId, 'Chat', 'rintawa/chat/v1', { method: 'recent', conversationId: conversation })).value;
    const send = async value => { await eventually(() => cdp.evaluate('!!document.querySelector("#message")&&!document.querySelector("#message").disabled', chat), 'composer ready'); await cdp.setInput('#message', value, chat); await cdp.click('Send', chat); };
    const waitIdle = async (timeout = 30000) => eventually(() => cdp.evaluate('!!document.querySelector("#message")&&!document.querySelector("#message").disabled', chat), 'generation terminal', timeout);
    await send('AI local request');
    await cdp.waitText('Local deterministic reply: ', chat);
    assert.equal(await cdp.evaluate('document.querySelector(".reply-preview")?.getAttribute("aria-busy") === "true"',chat),true);
    assert.equal((await recent()).items.some(row=>row.message.text.startsWith('Local deterministic reply:')),false,'prefix is not a World message');
    const stream=[...pending][0];assert.ok(stream);
    await cdp.evaluate('(()=>{const v=document.querySelector(".transcript");v.style.maxHeight="80px";v.scrollTop=0;})()',chat);
    const delta=Buffer.from(`data: ${JSON.stringify({choices:[{index:0,delta:{content:'При'},finish_reason:null}]})}\n\n`);
    const split=delta.indexOf(Buffer.from('При'))+1;
    stream.write(delta.subarray(0,split));await pause(30);stream.write(delta.subarray(split));
    await cdp.waitText('Local deterministic reply: При',chat);
    assert.equal(await cdp.evaluate('document.querySelector(".transcript").scrollTop',chat),0,'stream updates do not pull a reader away from older messages');
    await cdp.evaluate('(()=>{const v=document.querySelector(".transcript");v.style.maxHeight="";v.scrollTop=v.scrollHeight;})()',chat);
    await pause(300);assert.equal((await recent()).items.some(row=>row.message.text.startsWith('Local deterministic reply:')),false,'repeated status reads cannot COMMIT a prefix');
    await writeFile(join(root,'streaming-prefix.png'),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    emit(stream,'вет.');mode='normal';
    await cdp.waitText(text, chat); await waitIdle();
    await eventually(()=>cdp.evaluate('!document.querySelector(".reply-preview")',chat),'canonical answer replaces preview');
    assert.equal(await cdp.evaluate(`[...document.querySelectorAll('article.message p')].filter(p=>p.textContent===${JSON.stringify(text)}).length`,chat),1);
    checks.push('fragmented UTF-8 SSE renders cumulative Chat text before upstream completion and before COMMIT; final answer replaces its preview exactly once');
    let page = await recent(); assert.equal(page.items.at(-1).message.role, 'assistant'); assert.equal(page.items.at(-1).message.text, text); assert.equal(requests.length, 1);
    assert.ok(requests[0].body.messages[0].content.includes('Name: Story Alice'));assert.ok(Buffer.byteLength(requests[0].body.messages[0].content)>8192,'full character beyond former per-card cap reaches provider');assert.equal(requests[0].body.model,'local-fixture');
    assert.equal(requests[0].body.messages.at(-1).content, 'AI local request');
    assert.deepEqual(await describe(second,'Chat'),{ready:true,reason:null},'Chat sees readiness with no catalog or configuration fields');
    assert.ok(requests[0].body.messages.some(m=>m.role==='assistant'&&m.content==='Hello You, I am Story Alice.'));assert.ok(requests[0].body.messages[0].content.includes('Play Story Alice opposite You.'));assert.ok(requests[0].body.messages[0].content.includes('Keep the garden scene.'));
    assert.equal((await query(first.worldId, 'Chat', 'rintawa/inference/v1', { method: 'begin-generate', messages: [{ role: 'user', content: 'forged direct request' }] })).error.code, 'access-denied');
    assert.equal((await query(first.worldId, 'Story', 'rintawa/narrator/v1', { method: 'begin-generate', conversationId: conversation, userMessageId: page.items.at(-2).messageId, messageId: '00000000-0000-7000-8000-000000000006', entryId: '00000000-0000-7000-8000-000000000007' })).error.code, 'access-denied');
    assert.equal(requests.length, 1, 'forbidden callers never reach network');
    assert.equal((await query(first.worldId,'Chat','rintawa/inference/v1',{method:'set-default',expected:localConnection,expectedDefault:localConnection.id})).error.code,'access-denied');
    checks.push('first provider becomes primary automatically; Narrator prompt and loopback SSE commit one assistant reply under exact principals');
    checks.push('World permission recovery and consent preserve activation, frame and draft; A origin approval leaves B permissions unchanged');
    settings=await openSettings(second);
    await cdp.click('Add provider',settings);await cdp.click('OpenAI-compatible',settings);
    assert.equal(await cdp.evaluate('document.querySelector("#ai-auth").value === "bearer"',settings),true);
    await cdp.setInput('#ai-url',origin+'/v1',settings);await cdp.setInput('#ai-model','key-required-fixture',settings);await cdp.click('Save provider',settings);await saved(settings);await consentReady();await cdp.click('Cancel');await consentClosed();
    const sharedCatalog = await describe(null);
    assert.equal(sharedCatalog.connections.length,2);assert.equal(sharedCatalog.defaultConnectionId,localConnection.id);
    const keyConnection = sharedCatalog.connections.find(connection => connection.label === 'OpenAI-compatible');
    assert.ok(keyConnection);
    assert.deepEqual((await describe(first)).connections,sharedCatalog.connections);
    const keyPath=`ai.connection_${keyConnection.id.replaceAll('-','')}.api_key`;
    await clickConnection(settings,'OpenAI-compatible','Make primary');await waitPrimary(settings,'OpenAI-compatible');
    assert.equal((await describe(null)).defaultConnectionId,keyConnection.id);
    assert.equal((await describe(first)).defaultConnectionId,keyConnection.id);
    assert.deepEqual((await policy(second,bInstance)).secretRead,[]);
    await closeSettings();
    chat=await panel('.chat');await cdp.waitText('World B remains usable',chat);
    await cdp.setInput('#message','Message persists without credentials',chat);await cdp.click('Send',chat);await cdp.waitText('requires a local API key',chat);
    assert.equal(requests.length,1,'missing key approval never reaches network or reads a real vault value');
    const conversationB=(await query(second.worldId,'Chat','rintawa/chat/v1',{method:'list'})).value.items[0].id;
    const historyB=(await query(second.worldId,'Chat','rintawa/chat/v1',{method:'recent',conversationId:conversationB})).value;
    assert.equal(historyB.items.at(-1).message.text,'Message persists without credentials');assert.equal(historyB.items.at(-1).message.role,'user');
    settings=await openSettings(second);
    await clickConnection(settings,'Ollama','Make primary');await waitPrimary(settings,'Ollama');
    await clickConnection(settings,'Ollama','Allow access…');await consentReady();await cdp.click('Allow selected');await consentClosed();await cdp.waitText('Online',settings);
    assert.equal((await describe(null)).defaultConnectionId,localConnection.id);await closeSettings();
    await cdp.evaluate(`window.__storyAdmin.rpc('runtime-policy.apply-live-change',{change:{target:${JSON.stringify(beforeActivation)},delta:{allowSecretRead:[${JSON.stringify(keyPath)}]}}})`);
    assert.deepEqual((await policy(first,instance)).secretRead,[keyPath]);
    assert.ok((await readFile(join(home,'worlds',first.worldId,'composition.toml'),'utf8')).includes('[[secret_read_grants]]'));
    checks.push('B adds a provider and switches the shared primary; missing credentials leave its user message committed with zero provider requests');
    await cdp.openWorld('Story A');chat=await panel('.chat');await cdp.waitText(text,chat);

    let assistantCount = page.items.filter(row => row.message.role === 'assistant').length;
    for (const failure of ['http-error', 'incomplete', 'oversize', 'timeout']) {
      mode = failure;
      const beforeCount=(await recent()).items.filter(row=>row.message.role==='assistant').length;
      await send('AI failure test: '+failure);
      await eventually(() => requests.at(-1)?.mode === failure, `${failure} request`);
      if (failure === 'incomplete') {
        await cdp.waitText('Interrupted reply prefix',chat);
        assert.equal((await recent()).items.some(row=>row.message.text==='Interrupted reply prefix'),false);
        for(const response of pending)response.end();
      }
      await waitIdle(failure === 'timeout' ? 110000 : 30000);
      page=await recent();
      const retained=failure==='incomplete'||failure==='timeout';
      assert.equal(page.items.filter(row=>row.message.role==='assistant').length,beforeCount+(retained?1:0));
      if(retained){assert.equal(page.items.at(-1).message.text,failure==='incomplete'?'Interrupted reply prefix':'Timed out reply prefix');assert.equal(page.items.at(-1).message.role,'assistant');}
      if(failure==='timeout')await eventually(()=>pending.size===0,'deadline closes held HTTP');
      assert.ok(!(await cdp.body(chat)).includes('private upstream body must not be exposed'));
    }
    assistantCount=page.items.filter(row=>row.message.role==='assistant').length;
    checks.push('failed empty/oversized replies create no assistant; interrupted and timed-out valid prefixes commit ordinary messages without exposing upstream errors');
    mode = 'delayed'; await send('Concurrent context test'); await eventually(() => pending.size === 1, 'upstream generation held');
    const lastUser = (await recent()).items.at(-1).messageId;
    const spare = (await query(first.worldId, 'Chat', 'rintawa/chat/v1', { method: 'allocate', count: 2 })).value;
    assert.equal((await query(first.worldId, 'Chat', 'rintawa/narrator/v1', { method: 'begin-generate', conversationId: conversation, userMessageId: lastUser, messageId: spare[0], entryId: spare[1] })).error.code, 'generation-busy');
    const writeIds = (await query(first.worldId, 'Chat', 'rintawa/chat/v1', { method: 'allocate', count: 2 })).value;
    const later = (await query(first.worldId, 'Chat', 'rintawa/chat/v1', { method: 'begin-send', conversationId: conversation, messageId: writeIds[0], entryId: writeIds[1], text: 'Concurrent later user message' })).value;
    await eventually(async () => (await query(first.worldId, 'Chat', 'rintawa/chat/v1', { method: 'status', operation: later.operation })).value.status === 'committed', 'concurrent message COMMIT');
    await query(first.worldId, 'Chat', 'rintawa/chat/v1', { method: 'release', operation: later.operation });
    for (const response of pending) emit(response); await waitIdle(); await cdp.waitText('The conversation changed.', chat);
    page = await recent(); assert.equal(page.items.at(-1).message.text, 'Concurrent later user message'); assert.equal(page.items.filter(row => row.message.role === 'assistant').length, assistantCount);
    checks.push('one generation per conversation; a concurrent COMMIT rejects stale AI context without partial reply');
    await cdp.waitText('Concurrent later user message', chat);
    mode = 'delayed';
    const releasedIds = (await query(first.worldId, 'Chat', 'rintawa/chat/v1', { method: 'allocate', count: 2 })).value;
    const released = (await query(first.worldId, 'Chat', 'rintawa/narrator/v1', { method: 'begin-generate', conversationId: conversation, userMessageId: page.items.at(-1).messageId, messageId: releasedIds[0], entryId: releasedIds[1] })).value;
    assert.equal(released.status, 'pending');
    await eventually(async () => {
      try {
        const result = await query(first.worldId, 'Chat', 'rintawa/narrator/v1', { method: 'release', operation: released.operation });
        if (result.error?.code === 'unavailable' || result.error?.code === 'limit-exceeded') return false;
        assert.ok(!result.error || result.error.code === 'invalid-operation');
        return true;
      } catch (error) { if (/^(Error: )?(unavailable|limit-exceeded)(?:\n|$)/.test(error.message)) return false; throw error; }
    }, 'idempotent result cleanup after a transient lookup failure');
    assert.equal((await query(first.worldId, 'Chat', 'rintawa/narrator/v1', { method: 'status', operation: released.operation })).error.code, 'invalid-operation');
    assert.equal((await query(first.worldId, 'Chat', 'rintawa/narrator/v1', { method: 'begin-generate', conversationId: conversation, userMessageId: page.items.at(-1).messageId, messageId: spare[0], entryId: spare[1] })).error.code, 'generation-busy');
    await eventually(() => pending.size === 1, 'released pending generation still performs accepted work');
    const heldModel=requests.at(-1).body.model;
    settings=await openSettings(second);await clickConnection(settings,'Ollama','Edit');await cdp.setInput('#ai-model','updated-fixture',settings);await cdp.click('Save provider',settings);await saved(settings);await cdp.waitText('updated-fixture',settings);await cdp.waitText('Online',settings);
    const updatedCatalog=await describe(null);
    assert.equal(updatedCatalog.defaultConnectionId,localConnection.id,'model-only edits preserve the primary ID');
    assert.equal(updatedCatalog.connections.find(c=>c.id===localConnection.id).config.model,'updated-fixture');
    assert.deepEqual((await describe(first)).connections,updatedCatalog.connections);
    assert.deepEqual((await policy(second,bInstance)).target,bBefore.target);await closeSettings();
    chat=await panel('.chat');await cdp.waitText('Message persists without credentials',chat);
    mode='normal';await send('Shared connection B');await cdp.waitText(text,chat);await waitIdle();
    assert.equal(requests.at(-1).body.model,'updated-fixture');assert.equal(heldModel,'local-fixture');assert.equal(pending.size,1,'B request/stop-start did not cancel A');
    await cdp.openWorld('Story A');await stop(first);await eventually(() => pending.size === 0, 'World stop cancels owned HTTP');
    await cdp.openWorld('Story B');chat=await panel('.chat');await send('AI remains usable after A stop');
    const bRecent = async () => {
      try {
        const reply=await query(second.worldId,'Chat','rintawa/chat/v1',{method:'recent',conversationId:conversationB});
        if(reply.error?.code==='unavailable')return null;
        assert.equal(reply.error,undefined);return reply.value.items;
      }catch(error){if(/^(Error: )?unavailable(?:\n|$)/.test(error.message))return null;throw error;}
    };
    const finalBText='AI remains usable after A stop';
    const firstOutcome=await eventually(async()=>{
      const rows=await bRecent();
      if(rows?.at(-1).message.role==='assistant'&&rows.at(-2).message.text===finalBText)return 'committed';
      if((await cdp.body(chat)).includes('Reread World'))return 'unknown';
      return false;
    },'B write reaches a terminal state after A stop',30000);
    if(firstOutcome==='unknown') {
      // Exercise the existing user reconciliation flow. Never resend the original
      // transport query or assume that an absent snapshot proves rollback.
      const before=await eventually(bRecent,'B remains readable after A stop');
      await writeFile(join(root,'b-after-stop-reconciliation.json'),JSON.stringify({ui:await cdp.body(chat),before,requests:requests.length},null,2));
      assert.equal(before.some(row=>row.message.text===finalBText),false,'do not duplicate a committed user message');
      const callCount=requests.length;
      await cdp.click('Reread World',chat);await cdp.waitText('World data reread.',chat);
      await pause(500);assert.equal(requests.length,callCount,'rereading never replays generation');
      // A deliberate new user action, with newly allocated IDs, is allowed by the
      // uncertainty contract. The final history below must contain only one copy.
      await send(finalBText);
    }
    await eventually(async()=>{
      const rows=await bRecent();
      if(!rows)return false;
      return rows.at(-1).message.role==='assistant'&&rows.at(-2).message.text===finalBText;
    },'B still generates after A stop',30000);
    assert.equal((await bRecent()).filter(row=>row.message.text===finalBText).length,1);
    checks.push('a model edit in B updates the global primary for future requests; admitted A keeps the captured model and B continues generating after A stops');
    await cdp.openWorld('Story A');

    assert.equal((await worlds()).find(w => w.worldId === second.worldId).active, true);
    const countBeforeRestart = requests.length;
    await cdp.click('Start World'); await cdp.waitText('Stop World'); chat = await panel('.chat'); await cdp.waitText('Concurrent later user message', chat); await cdp.waitText(text, chat);
    await pause(500); assert.equal(requests.length, countBeforeRestart, 'restart never resumes or repeats generation');
    page = await recent(); assert.equal(page.items.filter(row => row.message.role === 'assistant').length, assistantCount);
    assert.equal((await query(first.worldId, 'Chat', 'rintawa/inference/v1', { method: 'describe' })).value.ready, true);
    checks.push('released pending work remains busy; stopping A closes its HTTP while B stays active; restart preserves the reply and resolves the shared primary without replay');
    await writeFile(join(root, 'ai-requests.json'), JSON.stringify(requests, null, 2));
    return { cleanup, origin, requests, instance, conversation, keyPath };
  } catch (failure) { await writeFile(join(root,'ai-requests.json'),JSON.stringify(requests,null,2)); await cleanup(); throw failure; }
}
