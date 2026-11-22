import assert from 'node:assert/strict';
import test from 'node:test';
import { Rpc, Client, BridgeError, handle, encodeBytes, decodeBytes, resolveView } from '../src/index.ts';

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
