/** Bounded invalidation hints + authoritative reads. No mutation replay or cache. */
import type { Client } from '@rintawa/bridge';
export type Observation = { refresh: () => Promise<void>; close: () => void };
export function observeResource<T>({ client, points = [], resource, read, apply, failed, interval = 5000 }: {
  client?: Client; points?: string[]; resource?: string;
  read: (signal: AbortSignal) => Promise<T>; apply: (value: T) => void;
  failed: (error: unknown) => void; interval?: number;
}): Observation {
  let closed = false, running = false, polling = false, dirty = true, started = false, failures = 0;
  let timer: ReturnType<typeof setTimeout> | undefined, receive: ReturnType<typeof setTimeout> | undefined;
  let blockedUntil = 0, wanted = 0, waitingFor = 0;
  let waiter: { promise: Promise<void>; resolve: () => void; reject: (error: unknown) => void } | null = null;
  const stop = new AbortController(), subscriptions = new Map<string, string>();
  const visible = () => (typeof document === 'undefined' || document.visibilityState !== 'hidden') && (client?.visible ?? true);
  const schedule = (delay: number) => {
    clearTimeout(timer);
    if (!closed && started && visible()) timer = setTimeout(() => void run(), Math.max(delay, blockedUntil - performance.now()));
  };
  const invalidate = () => { dirty = true; wanted++; if (!running) schedule(50); };
  const run = async () => {
    timer = undefined;
    if (closed || running || !visible()) return;
    running = true; dirty = false; const version = wanted;
    try {
      const value = await read(stop.signal);
      if (closed) return;
      if (!visible()) { dirty = true; return; }
      apply(value); failures = 0; blockedUntil = 0;
      if (waiter && version >= waitingFor) { waiter.resolve(); waiter = null; }
    } catch (error) {
      if (closed) return;
      failures = Math.min(failures + 1, 5);
      blockedUntil = performance.now() + Math.min(30000, 250 * 2 ** failures) * (1 + Math.random() * .2);
      failed(error); waiter?.reject(error); waiter = null;
    } finally {
      running = false;
      if (!closed) schedule(dirty ? 50 : failures ? Math.max(0, blockedUntil - performance.now()) : interval);
    }
  };
  const hint = (bytes: Uint8Array) => {
    if (bytes.length > 256) return;
    try {
      const value: unknown = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
      if (!value || typeof value !== 'object' || Array.isArray(value)) return;
      const v = value as Record<string, unknown>;
      if (v.v !== 1 || Object.keys(v).some(key => !['v', 'resource'].includes(key)) || !(v.resource === null || typeof v.resource === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(v.resource))) return;
      if (v.resource === null || !resource || v.resource === resource) invalidate();
    } catch { /* Invalid hints are inert; periodic reads still reconcile. */ }
  };
  const poll = async () => {
    receive = undefined;
    if (closed || polling || !visible() || !client) return;
    polling = true;
    try {
      for (const [point, handle] of subscriptions) {
        try { for (const bytes of await client.poll(handle)) hint(bytes); }
        catch { subscriptions.delete(point); void client.unsubscribe(handle).catch(() => {}); }
        if (closed || !visible()) break;
      }
    } finally {
      polling = false;
      if (!closed && visible() && subscriptions.size) receive = setTimeout(() => void poll(), 200);
    }
  };
  const visibility = () => {
    clearTimeout(timer); clearTimeout(receive);
    if (closed || !visible()) { dirty = true; return; }
    invalidate(); void poll();
  };
  const detach = client?.onVisibilityChanged(visibility);
  if (typeof window !== 'undefined') window.addEventListener('focus', visibility);
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', visibility);
  const refresh = () => {
    invalidate(); waitingFor = wanted;
    if (!waiter) {
      let resolve!: () => void, reject!: (error: unknown) => void;
      const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
      waiter = { promise, resolve, reject };
    }
    return waiter.promise;
  };
  // Subscribe first. A quota/failure only disables hints, never the source read.
  void (async () => {
    if (client) for (const point of [...new Set(points)].slice(0, 4)) {
      try {
        const handle = await client.subscribe(point);
        if (closed) { void client.unsubscribe(handle).catch(() => {}); break; }
        subscriptions.set(point, handle);
      } catch { /* Reconciliation is authoritative even without notifications. */ }
    }
    if (closed) return;
    started = true; schedule(0); void poll();
  })();
  return { refresh, close: () => {
    if (closed) return;
    closed = true; stop.abort(); clearTimeout(timer); clearTimeout(receive); detach?.();
    if (typeof window !== 'undefined') window.removeEventListener('focus', visibility);
    if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', visibility);
    waiter?.reject(new DOMException('Observation closed', 'AbortError')); waiter = null;
    for (const handle of subscriptions.values()) void client?.unsubscribe(handle).catch(() => {});
    subscriptions.clear();
  } };
}
