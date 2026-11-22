/** Single-authority application operations over the raw browser transport. */
import { BridgeError, type Client } from '@rintawa/bridge';
export type Operation = { operation: string; itemIds: string[] };
export type OperationStatus = { status: 'committed'; itemIds: string[] } | { status: 'failed'; reason: string } | { status: 'unknown'; itemIds: string[] };
type Pending = { status: 'pending' };
const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
const ids = (value: unknown): value is string[] => Array.isArray(value) && value.length <= 3 && value.every(id => typeof id === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(id));
export class DomainError extends Error {
  readonly code: string;
  readonly outcome: 'not-started' | 'unknown';
  constructor(code: string, outcome: 'not-started' | 'unknown' = 'unknown') { super(code); this.name = 'DomainError'; this.code = code; this.outcome = outcome; }
}
function cancelled(signal?: AbortSignal): void { if (signal?.aborted) throw new DOMException('Operation observation stopped', 'AbortError'); }
async function pause(signal?: AbortSignal): Promise<void> {
  cancelled(signal);
  await new Promise<void>((resolve, reject) => {
    const stop = () => { clearTimeout(timer); signal?.removeEventListener('abort', stop); reject(new DOMException('Operation observation stopped', 'AbortError')); };
    const timer = setTimeout(() => { signal?.removeEventListener('abort', stop); resolve(); }, 40);
    signal?.addEventListener('abort', stop, { once: true });
  });
}
export class DomainClient {
  readonly client: Pick<Client, 'query'>;
  readonly point: string;
  constructor(client: Pick<Client, 'query'>, point: string) { this.client = client; this.point = point; }
  async request<T = unknown>(method: string, args: Record<string, unknown> = {}): Promise<T> {
    if ('method' in args) throw new DomainError('invalid-request', 'not-started');
    const request = new TextEncoder().encode(JSON.stringify({ method, ...args }));
    if (request.length > 65536) throw new DomainError('message-too-large', 'not-started');
    const replies = await this.client.query(this.point, request);
    // Mutation outcomes cannot be inferred by picking one of several responses.
    if (replies.length !== 1) throw new DomainError(replies.length ? 'ambiguous-owner' : 'unavailable');
    if (replies[0].length > 65536) throw new DomainError('message-too-large');
    let value: unknown;
    try { value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(replies[0])); } catch { throw new DomainError('protocol-error'); }
    if (!object(value) || typeof value.ok !== 'boolean') throw new DomainError('protocol-error');
    if (value.ok) {
      if (Object.keys(value).some(key => !['ok', 'value'].includes(key)) || !('value' in value)) throw new DomainError('protocol-error');
      return value.value as T;
    }
    if (!object(value.error) || typeof value.error.code !== 'string') throw new DomainError('protocol-error');
    throw new DomainError(value.error.code, 'not-started');
  }
  async allocate(count: number): Promise<string[]> {
    if (!Number.isInteger(count) || count < 1 || count > 3) throw new DomainError('invalid-request', 'not-started');
    const result = await this.request('allocate', { count });
    if (!ids(result) || result.length !== count) throw new DomainError('protocol-error');
    return result;
  }
  async begin(method: string, args: Record<string, unknown>): Promise<Operation> {
    if (!method.startsWith('begin-')) throw new DomainError('invalid-request', 'not-started');
    const result = await this.request(method, args);
    if (!object(result) || result.status !== 'pending' || typeof result.operation !== 'string' || !/^[A-Za-z0-9_-]{43}$/.test(result.operation) || !ids(result.itemIds)) throw new DomainError('protocol-error');
    return { operation: result.operation, itemIds: result.itemIds };
  }
  async status(operation: Operation): Promise<OperationStatus | Pending> {
    const result = await this.request('status', { operation: operation.operation });
    if (!object(result)) throw new DomainError('protocol-error');
    if (result.status === 'pending') return { status: 'pending' };
    if (result.status === 'failed' && typeof result.reason === 'string' && new TextEncoder().encode(result.reason).length <= 2048) return { status: 'failed', reason: result.reason };
    if ((result.status === 'committed' || result.status === 'unknown') && ids(result.itemIds) && result.itemIds.length === operation.itemIds.length && result.itemIds.every((id, index) => id === operation.itemIds[index])) return { status: result.status, itemIds: result.itemIds };
    throw new DomainError('protocol-error');
  }
  async release(operation: Operation): Promise<void> {
    for (let attempt = 0; ; attempt++) {
      try { await this.request('release', { operation: operation.operation }); return; }
      catch (error) {
        const code = error instanceof DomainError || error instanceof BridgeError ? error.code : null;
        // A lost cleanup acknowledgement can leave the handle already retired.
        if (code === 'invalid-operation') return;
        if (attempt >= 4 || (code !== 'unavailable' && code !== 'limit-exceeded')) throw error;
        await pause();
      }
    }
  }
  async wait(operation: Operation, signal?: AbortSignal): Promise<OperationStatus> {
    for (;;) {
      cancelled(signal);
      let status: OperationStatus | Pending;
      try { status = await this.status(operation); }
      catch (error) {
        // A status lookup is read-only. Beginning the operation is never retried.
        const code = error instanceof DomainError || error instanceof BridgeError ? error.code : null;
        if (code !== 'unavailable' && code !== 'limit-exceeded') throw error;
        await pause(signal); continue;
      }
      cancelled(signal);
      if (status.status === 'pending') { await pause(signal); continue; }
      // The UI already knows this terminal outcome even if cleanup's reply is lost.
      void this.release(operation).catch(() => {});
      return status;
    }
  }
}
