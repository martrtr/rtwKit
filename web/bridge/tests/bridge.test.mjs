import assert from 'node:assert/strict';
import test from 'node:test';
import { Rpc, Client, BridgeError, handle, encodeBytes, decodeBytes, resolveView, viewState, RootBridge } from '../src/index.ts';

test('u64 values remain strings and byte payloads survive the full bound', () => {
  assert.equal(handle('18446744073709551615'), '18446744073709551615');
  for (const value of [1, '01', '-1', '18446744073709551616']) assert.throws(() => handle(value), BridgeError);
  const bytes = Uint8Array.from({ length: 65536 }, (_, i) => i % 256);
  assert.deepEqual(decodeBytes(encodeBytes(bytes)), bytes);
});
test('socket responses are bound to their original channel; panel replies omit it', async () => {
  const sent = []; let closed = false; const rpc = new Rpc(value => sent.push(value), 200, () => { closed = true; });
  const a = rpc.request('transport.poll-query', { handle: '1' }, 'a');
  const b = rpc.request('transport.poll-query', { handle: '2' }, 'b');
  rpc.accept({ v: 1, id: sent[1].id, channel: 'b', ok: true, value: [] });
  rpc.accept({ v: 1, id: sent[0].id, channel: 'a', ok: true, value: null });
  assert.deepEqual(await b, []); assert.equal(await a, null);
  const foreign = rpc.request('ui.layout.get', {}, 'a');
  rpc.accept({ v: 1, id: sent[2].id, channel: 'b', ok: true, value: 'forged' });
  await assert.rejects(foreign, error => error.code === 'protocol-error' && error.outcome === 'unknown');
  assert.equal(closed, true);
});
test('timeouts never replay a request and quotas recover', async () => {
  const sent = []; const rpc = new Rpc(value => sent.push(value), 15);
  const promises = Array.from({ length: 8 }, () => rpc.request('mutation', {}, 'a').catch(error => error));
  await assert.rejects(rpc.request('ninth', {}, 'a'), error => error.code === 'limit-exceeded' && error.outcome === 'not-started');
  const errors = await Promise.all(promises); assert.equal(sent.length, 8);
  assert.ok(errors.every(error => error.code === 'unavailable' && error.outcome === 'unknown'));
  const fresh = rpc.request('fresh', {}, 'a');
  rpc.accept({ v: 1, id: sent.at(-1).id, channel: 'a', ok: true, value: 'ok' }); assert.equal(await fresh, 'ok'); rpc.close();
});
test('queries return an empty terminal list and never reinterpret it as a commit', async () => {
  const calls = [];
  const client = new Client(async (method, args) => { calls.push([method, args]); return method === 'transport.begin-query' ? '3' : []; });
  assert.deepEqual(await client.query('domain/read', new Uint8Array()), []);
  assert.deepEqual(calls.map(([method]) => method), ['transport.begin-query', 'transport.poll-query']); client.dispose();
});
test('restore uses the complete stable key and never selects by title or first match', () => {
  const key = { worldId: 'world-a', instanceId: 'one', componentId: 'panel' };
  const first = { id: 'new-live', title: 'Same title', worldId: 'world-a', restoreKey: key };
  const second = { id: 'other', title: 'Same title', worldId: 'world-b', restoreKey: { ...key, worldId: 'world-b' } };
  assert.equal(resolveView(key, [second, first]), first);
  assert.equal(resolveView(key, [first, { ...first, id: 'ambiguous' }]), null);
  assert.equal(resolveView({ ...key, instanceId: 'new-install' }, [first]), null);
});
test('disposing a channel aborts handlers and releases a non-cooperating query loop', async () => {
  const calls = []; let entered; const started = new Promise(resolve => { entered = resolve; }); let signal;
  const client = new Client(async (method) => {
    calls.push(method);
    if (method === 'transport.declare-polled') return '1';
    return { handle: '2', caller: { instanceId: 'caller', componentId: 'dice', activation: '3' }, payload: '' };
  });
  const service = await client.serve('fixture/dispose', (_query, cancellation) => {
    signal = cancellation; entered(); return new Promise(() => {});
  });
  await started; client.dispose();
  assert.equal(signal.aborted, true);
  await service.done;
  assert.deepEqual(calls, ['transport.declare-polled', 'transport.next-query']);
});

test('a poll rejected before dispatch cannot undo an already-admitted query', async () => {
  const calls = [];
  const client = new Client(async method => {
    calls.push(method);
    if (method === 'transport.begin-query') return '1';
    if (method === 'transport.poll-query') throw new BridgeError('limit-exceeded', 'not-started');
  });
  await assert.rejects(client.query('domain/mutate', new Uint8Array()), error => error.code === 'limit-exceeded' && error.outcome === 'unknown');
  assert.deepEqual(calls, ['transport.begin-query', 'transport.poll-query', 'transport.close-query']);
});

test('view selection state is bounded inert strings and never selects preference authority', async () => {
  const state = { conversationId: '018f0000-0000-7000-8000-000000000001' };
  assert.equal(viewState(state), true);
  for (const bad of [{ nested: {} }, { value: 'é'.repeat(129) }, { ['x'.repeat(65)]: 'id' }, Object.fromEntries(Array.from({length:9}, (_,i)=>[String(i),'id'])), { a:'x'.repeat(256),b:'x'.repeat(256),c:'x'.repeat(256),d:'x'.repeat(256) }]) assert.equal(viewState(bad), false);
  const calls=[];const client=new Client(async(method,args)=>{calls.push({method,args});return method==='ui.state.get'?state:null;});
  assert.deepEqual(await client.getState(),state);await client.setState(state);
  assert.deepEqual(calls,[{method:'ui.state.get',args:{}},{method:'ui.state.set',args:{state}}]);
  await assert.rejects(client.setState({nested:{}}),error=>error.code==='invalid-request');
});

test('a cancelled pending mount never inserts an iframe or disconnects other views when its channel already closed', async () => {
  const prior=globalThis.window;globalThis.window=new EventTarget();
  let acknowledge,entered;const ready=new Promise(resolve=>{entered=resolve;});let created=0,closed=0;const calls=[];
  const socket=new EventTarget();socket.readyState=1;socket.close=()=>{closed++;};
  const view={id:'view',worldId:'018f0000-0000-7000-8000-000000000001',title:'Chat',restoreKey:{worldId:'018f0000-0000-7000-8000-000000000001',instanceId:'instance',componentId:'ui'}};
  const rpc={request:async(method)=>{calls.push(method);if(method==='ui.list')return [view];if(method==='ui.mount'){entered();return new Promise(resolve=>{acknowledge=resolve;});}if(method==='ui.unmount')throw new BridgeError('channel-closed','not-started');},close:()=>{}};
  const bridge=new RootBridge(socket,rpc,'root');const abort=new AbortController();
  try {
    const result=bridge.mount('view',{append:()=>{created++;}},()=>{}, {},abort.signal);
    await ready;abort.abort();acknowledge({channel:'retired',url:'/bundle/view/index.html'});
    await assert.rejects(result,error=>error.name==='AbortError');
    assert.equal(created,0);assert.equal(closed,0);assert.deepEqual(calls,['ui.list','ui.mount','ui.unmount']);
  } finally {bridge.disconnect();globalThis.window=prior;}
});

test('sourced polling preserves native principal and never obtains identity from payload', async () => {
  const calls=[];const provider={instanceId:'a',componentId:'view',activation:'7'};
  const payload=new TextEncoder().encode('{"instanceId":"forged","title":"Example"}');
  const client=new Client(async(method,args)=>{calls.push([method,args]);if(method==='transport.begin-query')return '8';return [{provider,payload:encodeBytes(payload)}];});
  const replies=await client.querySourced('extension/sections',new Uint8Array());
  assert.deepEqual(replies,[{provider,payload}]);
  assert.deepEqual(calls.map(([method])=>method),['transport.begin-query','transport.poll-query-sourced']);
  client.dispose();
});
