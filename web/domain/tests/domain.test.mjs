import test from 'node:test';
import assert from 'node:assert/strict';
import { BridgeError } from '../../bridge/src/index.ts';
import { DomainClient, DomainError } from '../src/index.ts';
const id = '00000000-0000-7000-8000-000000000001';
const operation = { operation: 'x'.repeat(43), itemIds: [id] };
const reply = value => [new TextEncoder().encode(JSON.stringify({ ok: true, value }))];

test('ambiguous mutation replies are rejected without selecting a winner or replaying', async () => {
  let calls = 0;
  const domain = new DomainClient({ query: async () => { calls++; return [...reply({}), ...reply({})]; } }, 'owner/v1');
  await assert.rejects(domain.begin('begin-create', {}), error => error instanceof DomainError && error.code === 'ambiguous-owner' && error.outcome === 'unknown');
  assert.equal(calls, 1);
});
test('lost status response is retryable and terminal cleanup never replays begin', async () => {
  const calls = []; let statuses = 0;
  const domain = new DomainClient({ query: async (_point, bytes) => {
    const request = JSON.parse(new TextDecoder().decode(bytes)); calls.push(request.method);
    if (request.method === 'begin-create') return reply({ status: 'pending', ...operation });
    if (request.method === 'release') throw new BridgeError('unavailable');
    if (++statuses === 1) throw new BridgeError('unavailable');
    return reply(statuses === 2 ? { status: 'pending' } : { status: 'committed', itemIds: [id] });
  } }, 'owner/v1');
  const admitted = await domain.begin('begin-create', { conversationId: id });
  assert.deepEqual(await domain.wait(admitted), { status: 'committed', itemIds: [id] });
  assert.deepEqual(calls, ['begin-create', 'status', 'status', 'status', 'release']);
});
test('lost begin remains unknown with caller-known IDs and is never retried', async () => {
  let calls = 0;
  const domain = new DomainClient({ query: async () => { calls++; throw new BridgeError('unavailable'); } }, 'owner/v1');
  await assert.rejects(domain.begin('begin-create', { conversationId: id }), error => error.outcome === 'unknown');
  assert.equal(calls, 1);
});
test('status cannot substitute another operation result and release-invalid status is explicit', async () => {
  const domain = new DomainClient({ query: async () => reply({ status: 'committed', itemIds: [] }) }, 'owner/v1');
  await assert.rejects(domain.status(operation), error => error.code === 'protocol-error');
  const closed = new DomainClient({ query: async () => [new TextEncoder().encode(JSON.stringify({ ok: false, error: { code: 'invalid-operation' } }))] }, 'owner/v1');
  await assert.rejects(closed.wait(operation), error => error.code === 'invalid-operation');
});

test('terminal release recovers from local rejection and a lost cleanup acknowledgement', async () => {
  const calls = [];
  const domain = new DomainClient({ query: async (_point, bytes) => {
    const request = JSON.parse(new TextDecoder().decode(bytes)); calls.push(request.method);
    assert.equal(request.operation, operation.operation);
    if (calls.length === 1) throw new BridgeError('limit-exceeded', 'not-started');
    if (calls.length === 2) throw new BridgeError('unavailable');
    return [new TextEncoder().encode(JSON.stringify({ ok: false, error: { code: 'invalid-operation' } }))];
  } }, 'owner/v1');
  await domain.release(operation);
  assert.deepEqual(calls, ['release', 'release', 'release']);
});

test('cumulative pending text survives missed polls and is delivered once per changed snapshot before COMMIT', async () => {
  const states = [{status:'pending',text:'При'}, {status:'pending',text:'При'}, {status:'pending'}, {status:'pending',text:'Привет 🌍'}, {status:'committed',itemIds:[id]}];
  const calls=[], texts=[];
  const domain=new DomainClient({query:async(_point,bytes)=>{const q=JSON.parse(new TextDecoder().decode(bytes));calls.push(q.method);return reply(q.method==='release'?null:states.shift());}},'owner/v1');
  assert.deepEqual(await domain.wait(operation,undefined,text=>texts.push(text)),{status:'committed',itemIds:[id]});
  assert.deepEqual(texts,['При','Привет 🌍']);assert.equal(calls.filter(c=>c.startsWith('begin-')).length,0);
});
test('invalid or rewritten snapshots fail closed; abort cannot deliver a late prefix', async () => {
  for(const text of [42,'я'.repeat(8193)]){
    const domain=new DomainClient({query:async()=>reply({status:'pending',text})},'owner/v1');
    await assert.rejects(domain.status(operation),e=>e.code==='protocol-error');
  }
  let reads=0;const rewritten=new DomainClient({query:async()=>reply({status:'pending',text:++reads===1?'first':'other'})},'owner/v1');
  await assert.rejects(rewritten.wait(operation),e=>e.code==='protocol-error');
  const controller=new AbortController(),text=[];
  const late=new DomainClient({query:async()=>{controller.abort();return reply({status:'pending',text:'late'});}},'owner/v1');
  await assert.rejects(late.wait(operation,controller.signal,v=>text.push(v)),e=>e.name==='AbortError');assert.deepEqual(text,[]);
});
