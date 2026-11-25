/** General Rintawa browser transport. A port binds authority; payloads never select it. */
export type Outcome = 'not-started' | 'unknown';
export class BridgeError extends Error {
  code: string;
  outcome: Outcome;
  constructor(code: string, outcome: Outcome = 'unknown') {
    super(code); this.name = 'BridgeError'; this.code = code; this.outcome = outcome;
  }
}
export type RestoreKey = { worldId: string | null; instanceId: string; componentId: string };
export type View = { id: string; title: string; activation: string; launchable: boolean; provides: { id: string; version: number; selected: boolean }[]; worldId: string | null; restoreKey: RestoreKey };
export type Caller = { instanceId: string; componentId: string; activation: string };
export type SourcedReply = { provider: Caller; payload: Uint8Array };
export type InboundQuery = { handle: string; caller: Caller; payload: Uint8Array };
export type AssetRef = { digest: string; size: string; mediaType: string };
export type ResourceRef = { id: string; size: string; mediaType: string; name: string | null };
export type ViewState = Record<string, string>;
export function viewState(value: unknown): value is ViewState {
  return record(value) && Object.keys(value).length <= 8 && Object.entries(value).every(([key, value]) => asciiId(key) && typeof value === 'string' && new TextEncoder().encode(value).length <= 256) && new TextEncoder().encode(JSON.stringify(value)).length <= 1024;
}
export type PermissionRequest = { componentId: string; runtime: string[]; httpOrigins: string[]; secretRead: string[] };
export type Theme = { mode: 'light' | 'dark' };
type Call = (method: string, args: Record<string, unknown>) => Promise<unknown>;
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
const asciiId = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && value.length <= 64 && /^[\x00-\x7f]+$/.test(value);
const delay = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));
export function handle(value: unknown): string {
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value) || value.length > 20 || BigInt(value) > 18446744073709551615n) throw new BridgeError('protocol-error');
  return value;
}
export function encodeBytes(bytes: Uint8Array): string {
  let binary = ''; for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return btoa(binary);
}
export function decodeBytes(value: unknown): Uint8Array<ArrayBuffer> {
  if (typeof value !== 'string') throw new BridgeError('protocol-error');
  try { return Uint8Array.from(atob(value), c => c.charCodeAt(0)); } catch { throw new BridgeError('protocol-error'); }
}
export function resolveView(key: RestoreKey, views: View[]): View | null {
  const matches = views.filter(v => v.restoreKey.worldId === key.worldId && v.restoreKey.instanceId === key.instanceId && v.restoreKey.componentId === key.componentId);
  return matches.length === 1 ? matches[0] : null;
}

/** Shared connection correlator; the shell generates IDs independently of panel input IDs. */
export class Rpc {
  #send: (value: unknown) => void;
  #pending = new Map<string, { channel?: string; resolve: (value: unknown) => void; reject: (error: BridgeError) => void; timer: ReturnType<typeof setTimeout> }>();
  #sequence = 0n;
  #closed = false;
  #timeout: number;
  #onClose: () => void;
  constructor(send: (value: unknown) => void, timeout = 2000, onClose: () => void = () => {}) { this.#send = send; this.#timeout = timeout; this.#onClose = onClose; }
  request(method: string, args: Record<string, unknown>, channel?: string): Promise<unknown> {
    if (this.#closed) return Promise.reject(new BridgeError('channel-closed', 'not-started'));
    const own = [...this.#pending.values()].filter(p => p.channel === channel).length;
    if (this.#pending.size >= 64 || own >= 8) return Promise.reject(new BridgeError('limit-exceeded', 'not-started'));
    const id = (++this.#sequence).toString();
    if (!asciiId(id)) return Promise.reject(new BridgeError('limit-exceeded', 'not-started'));
    const message = { v: 1, id, ...(channel === undefined ? {} : { channel }), method, args };
    if (new TextEncoder().encode(JSON.stringify(message)).length > 96 * 1024) return Promise.reject(new BridgeError('message-too-large', 'not-started'));
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.#pending.delete(id); reject(new BridgeError('unavailable')); }, this.#timeout);
      this.#pending.set(id, { channel, resolve, reject, timer });
      try { this.#send(message); } catch { clearTimeout(timer); this.#pending.delete(id); reject(new BridgeError('channel-closed')); }
    });
  }
  accept(value: unknown): void {
    if (!record(value) || value.v !== 1 || !asciiId(value.id) || typeof value.ok !== 'boolean') { this.close('protocol-error'); return; }
    const pending = this.#pending.get(value.id); if (!pending) return;
    if ((pending.channel !== undefined && value.channel !== pending.channel) || (pending.channel === undefined && 'channel' in value)) { this.close('protocol-error'); return; }
    clearTimeout(pending.timer); this.#pending.delete(value.id);
    if (value.ok) pending.resolve(value.value);
    else if (record(value.error) && typeof value.error.code === 'string' && ['not-started', 'unknown'].includes(String(value.error.outcome))) pending.reject(new BridgeError(value.error.code, value.error.outcome as Outcome));
    else pending.reject(new BridgeError('protocol-error'));
  }
  close(code = 'channel-closed'): void {
    if (this.#closed) return;
    this.#closed = true; for (const pending of this.#pending.values()) { clearTimeout(pending.timer); pending.reject(new BridgeError(code)); } this.#pending.clear(); this.#onClose();
  }
}

export class Client {
  #call: Call;
  #closed = false;
  #urls = new Set<string>();
  #stop = new AbortController();
  #policyListeners = new Set<() => void>();
  #visibilityListeners = new Set<() => void>();
  visible = true;
  onVisibilityChanged(handler: () => void): () => void { this.#visibilityListeners.add(handler); return () => { this.#visibilityListeners.delete(handler); }; }
  visibilityChanged(visible: boolean): void { if (this.visible === visible) return; this.visible = visible; for (const handler of this.#visibilityListeners) handler(); }
  #slots = new Map<string, () => void>();
  onPermissionsChanged(handler: () => void): () => void { this.#policyListeners.add(handler); return () => { this.#policyListeners.delete(handler); }; }
  permissionsChanged(): void { if (!this.#closed) for (const handler of this.#policyListeners) handler(); }
  readonly initialState: ViewState;
  constructor(call: Call, initialState: ViewState = {}) { this.#call = call; this.initialState = initialState; }
  async getState(): Promise<ViewState> { const state = await this.request('ui.state.get'); if (!viewState(state)) throw new BridgeError('invalid-stored-data'); return state; }
  async setState(state: ViewState): Promise<void> { if (!viewState(state)) throw new BridgeError('invalid-request', 'not-started'); await this.request('ui.state.set', { state }); }
  async navigate(instanceId: string, componentId: string, state: ViewState = {}): Promise<void> { if (!viewState(state)) throw new BridgeError('invalid-request', 'not-started'); await this.request('ui.navigate', { instanceId, componentId, state }); }
  navigateView(view: View, state: ViewState = {}): Promise<unknown> { if (!viewState(state)) return Promise.reject(new BridgeError('invalid-request','not-started')); return this.request('ui.navigate', { instanceId:view.restoreKey.instanceId, componentId:view.restoreKey.componentId, activation:view.activation, state }); }
  request(method: string, args: Record<string, unknown> = {}): Promise<unknown> {
    return this.#closed ? Promise.reject(new BridgeError('channel-closed', 'not-started')) : this.#call(method, args);
  }
  async requestSecret(componentId: string, slot: string): Promise<string> {
    const admitted = await this.request('secrets.request-selection', {componentId, slot}) as {reference:string};
    try {
      for (;;) {
        let status: {status:string;result?:{outcome:string};code?:string};
        try {status = await this.request('secret-prompt.status', {reference:admitted.reference}) as typeof status;}
        catch(failure){if(failure instanceof BridgeError&&failure.code==='unavailable'){await delay(150);continue;}throw failure;}
        if (status.status === 'completed') return status.result!.outcome;
        if (status.status === 'failed') throw new BridgeError(status.code ?? 'unavailable');
        await delay(150);
      }
    } catch(failure) {
      if(failure instanceof BridgeError)return ['invalid-handle','not-found'].includes(failure.code)?'cancelled':failure.code==='permission-denied'?'stale':'unknown';
      return 'unavailable';
    } finally { await this.request('secret-prompt.release', {reference:admitted.reference}).catch(()=>{}); }
  }
  async beginQuery(point: string, payload: Uint8Array): Promise<string> {
    if (payload.length > 65536) throw new BridgeError('message-too-large', 'not-started');
    return handle(await this.request('transport.begin-query', { point, payload: encodeBytes(payload) }));
  }
  async beginQueryTo(point: string, payload: Uint8Array, expected: Caller): Promise<string> {
    if (payload.length > 65536) throw new BridgeError('message-too-large', 'not-started');
    if (!expected || typeof expected.instanceId !== 'string' || typeof expected.componentId !== 'string' || !expected.instanceId || !expected.componentId || expected.instanceId.length > 256 || expected.componentId.length > 256) throw new BridgeError('invalid-request', 'not-started');
    return handle(await this.request('transport.begin-query-to', { point, payload: encodeBytes(payload), expected: { instanceId: expected.instanceId, componentId: expected.componentId, activation: handle(expected.activation) } }));
  }
  async queryTo(point: string, payload: Uint8Array, expected: Caller, signal?: AbortSignal): Promise<Uint8Array[]> {
    if (signal?.aborted) throw new DOMException('Query observation stopped', 'AbortError');
    const query = await this.beginQueryTo(point, payload, expected);
    try {
      for (;;) {
        if (signal?.aborted) throw new DOMException('Query observation stopped', 'AbortError');
        const replies = await this.request('transport.poll-query-sourced', { handle: query });
        if (replies === null) { await delay(20); continue; }
        if (!Array.isArray(replies) || replies.some(reply => !record(reply) || !record(reply.provider) || reply.provider.instanceId !== expected.instanceId || reply.provider.componentId !== expected.componentId || reply.provider.activation !== expected.activation)) throw new BridgeError('unavailable');
        return replies.map(reply => decodeBytes(reply.payload));
      }
    } catch (error) {
      void this.closeQuery(query).catch(() => {});
      if (error instanceof BridgeError) throw new BridgeError(error.code, 'unknown');
      throw error;
    }
  }
  async pollQuery(query: string): Promise<Uint8Array[] | null> {
    const result = await this.request('transport.poll-query', { handle: handle(query) });
    if (result === null) return null;
    if (!Array.isArray(result)) throw new BridgeError('protocol-error');
    return result.map(decodeBytes);
  }
  closeQuery(query: string): Promise<unknown> { return this.request('transport.close-query', { handle: handle(query) }); }
  async query(point: string, payload: Uint8Array, signal?: AbortSignal): Promise<Uint8Array[]> {
    if (signal?.aborted) throw new DOMException('Query observation stopped', 'AbortError');
    const query = await this.beginQuery(point, payload);
    try {
      for (;;) {
        if (signal?.aborted) throw new DOMException('Query observation stopped', 'AbortError');
        const result = await this.pollQuery(query);
        if (result !== null) return result;
        await delay(20);
      }
    } catch (error) {
      // Releasing a receiver does not cancel or retry the provider's effects.
      void this.closeQuery(query).catch(() => {});
      // The query was admitted. A later poll can be rejected before dispatch,
      // but that says nothing about the provider's already-accepted work.
      if (error instanceof BridgeError) throw new BridgeError(error.code, 'unknown');
      throw error;
    }
  }
  async querySourced(point: string, payload: Uint8Array, signal?: AbortSignal): Promise<SourcedReply[]> {
    if (signal?.aborted) throw new DOMException('Query observation stopped', 'AbortError');
    const query = await this.beginQuery(point, payload);
    try {
      for (;;) {
        if (signal?.aborted) throw new DOMException('Query observation stopped', 'AbortError');
        const replies = await this.request('transport.poll-query-sourced', { handle: query });
        if (replies === null) { await delay(20); continue; }
        if (!Array.isArray(replies)) throw new BridgeError('protocol-error');
        return replies.map(reply => {
          if (!record(reply) || !record(reply.provider) || typeof reply.provider.instanceId !== 'string' || typeof reply.provider.componentId !== 'string') throw new BridgeError('protocol-error');
          return { provider: { instanceId: reply.provider.instanceId, componentId: reply.provider.componentId, activation: handle(reply.provider.activation) }, payload: decodeBytes(reply.payload) };
        });
      }
    } catch (error) {
      void this.closeQuery(query).catch(() => {});
      if (error instanceof BridgeError) throw new BridgeError(error.code, 'unknown');
      throw error;
    }
  }
  async views(): Promise<View[]> { return await this.request('ui.views') as View[]; }
  slotClosed(slot: string): void { this.#slots.get(slot)?.(); this.#slots.delete(slot); }
  async mountView(view: View, element: HTMLElement, state: ViewState = {}, onClosed: () => void = () => {}): Promise<{ close: () => Promise<void> }> {
    if (!viewState(state)) throw new BridgeError('invalid-request', 'not-started');
    const slot = [...crypto.getRandomValues(new Uint8Array(12))].map(v => v.toString(16).padStart(2, '0')).join('');
    const ancestors: HTMLElement[] = [];
    for (let parent = element.parentElement; parent; parent = parent.parentElement) ancestors.push(parent);
    const geometry = () => {
      const r = element.getBoundingClientRect();
      let left = 0, top = 0, right = innerWidth, bottom = innerHeight;
      for (const parent of ancestors) {
        const style = getComputedStyle(parent), bounds = parent.getBoundingClientRect();
        if (/^(hidden|clip|auto|scroll)$/.test(style.overflowX)) { left = Math.max(left, bounds.left + parent.clientLeft); right = Math.min(right, bounds.left + parent.clientLeft + parent.clientWidth); }
        if (/^(hidden|clip|auto|scroll)$/.test(style.overflowY)) { top = Math.max(top, bounds.top + parent.clientTop); bottom = Math.min(bottom, bounds.top + parent.clientTop + parent.clientHeight); }
        if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') right = left;
      }
      return { rect: { x: r.x, y: r.y, width: r.width, height: r.height }, clip: { x: left, y: top, width: Math.max(0, right - left), height: Math.max(0, bottom - top) } };
    };
    let closed = false, scheduled = false, updating = false, dirty = false;
    let closing: Promise<unknown> | undefined;
    const closeSlot = () => closing ??= (async () => {
      for (let attempt = 0; ; attempt++) {
        try { return await this.request('ui.slot.close', { slot }); }
        catch (error) {
          if (this.#closed || (error instanceof BridgeError && error.code === 'channel-closed')) return;
          if (!(error instanceof BridgeError) || !['limit-exceeded', 'unavailable'].includes(error.code) || attempt >= 40) throw error;
          await delay(25);
        }
      }
    })();
    const update = () => {
      if (closed) return;
      dirty = true;
      if (scheduled || updating) return;
      scheduled = true;
      requestAnimationFrame(() => {
        scheduled = false;
        if (closed) return;
        dirty = false; updating = true;
        void this.request('ui.slot.update', { slot, ...geometry() }).catch(() => {
          cleanup(); void closeSlot().catch(() => {});
        }).finally(() => { updating = false; if (dirty && !closed) update(); });
      });
    };
    const observer = new ResizeObserver(update);
    const cleanup = () => { if (closed) return; closed = true; observer.disconnect(); window.removeEventListener('scroll', update, true); window.removeEventListener('resize', update); this.#slots.delete(slot); onClosed(); };
    this.#slots.set(slot, cleanup);
    try {
      await this.request('ui.slot.mount', { slot, reference: { instanceId: view.restoreKey.instanceId, componentId: view.restoreKey.componentId, activation: view.activation }, ...geometry(), state });
      if (closed || this.#closed) { void closeSlot().catch(() => {}); throw new BridgeError('channel-closed'); }
      observer.observe(element); for (const parent of ancestors) observer.observe(parent); window.addEventListener('scroll', update, true); window.addEventListener('resize', update);
      return { close: async () => { cleanup(); await closeSlot(); } };
    } catch (error) { cleanup(); void closeSlot().catch(() => {}); throw error; }
  }
  async declarePolled(point: string): Promise<string> { return handle(await this.request('transport.declare-polled', { point })); }
  async nextQuery(queryable: string): Promise<InboundQuery | null> {
    const result = await this.request('transport.next-query', { queryable: handle(queryable) });
    if (result === null) return null;
    if (!record(result) || !record(result.caller) || typeof result.caller.instanceId !== 'string' || typeof result.caller.componentId !== 'string') throw new BridgeError('protocol-error');
    return { handle: handle(result.handle), caller: { instanceId: result.caller.instanceId, componentId: result.caller.componentId, activation: handle(result.caller.activation) }, payload: decodeBytes(result.payload) };
  }
  replyQuery(query: string, payload: Uint8Array | null): Promise<unknown> {
    if (payload && payload.length > 65536) return Promise.reject(new BridgeError('message-too-large', 'not-started'));
    return this.request('transport.reply-query', { handle: handle(query), payload: payload === null ? null : encodeBytes(payload) });
  }
  closeQueryable(queryable: string): Promise<unknown> { return this.request('transport.close-queryable', { handle: handle(queryable) }); }
  async serve(point: string, handler: (query: InboundQuery, signal: AbortSignal) => Promise<Uint8Array | null> | Uint8Array | null): Promise<{ done: Promise<void>; close: () => Promise<void> }> {
    const queryable = await this.declarePolled(point); const stop = new AbortController();
    const signal = AbortSignal.any([stop.signal, this.#stop.signal]);
    const done = (async () => {
      while (!stop.signal.aborted && !this.#stop.signal.aborted) {
        let query: InboundQuery | null;
        try { query = await this.nextQuery(queryable); }
        catch (error) {
          if (error instanceof BridgeError && error.outcome === 'not-started' && error.code === 'limit-exceeded') { await delay(25); continue; }
          if (stop.signal.aborted || this.#stop.signal.aborted) return;
          throw error;
        }
        if (query) {
          let payload: Uint8Array | null = null;
          // A closed channel must release this polling loop even if application code
          // ignores cancellation. The signal also lets cooperating handlers release work.
          payload = await new Promise<Uint8Array | null>(resolve => {
            const aborted = () => resolve(null);
            signal.addEventListener('abort', aborted, { once: true });
            if (signal.aborted) { signal.removeEventListener('abort', aborted); resolve(null); return; }
            Promise.resolve().then(() => handler(query!, signal)).then(resolve, () => resolve(null))
              .finally(() => signal.removeEventListener('abort', aborted));
          });
          if (stop.signal.aborted || this.#stop.signal.aborted) return;
          try { await this.replyQuery(query.handle, payload); }
          catch (error) { if (!(error instanceof BridgeError && error.code === 'invalid-handle')) throw error; }
        }
        await delay(25);
      }
    })();
    return { done, close: async () => { stop.abort(); await this.closeQueryable(queryable); } };
  }
  publish(point: string, payload: Uint8Array): Promise<unknown> { return this.request('transport.publish', { point, payload: encodeBytes(payload) }); }
  async subscribe(point: string): Promise<string> { return handle(await this.request('transport.subscribe', { point })); }
  async poll(subscription: string): Promise<Uint8Array[]> { const value = await this.request('transport.poll', { handle: handle(subscription) }); if (!Array.isArray(value)) throw new BridgeError('protocol-error'); return value.map(decodeBytes); }
  unsubscribe(subscription: string): Promise<unknown> { return this.request('transport.unsubscribe', { handle: handle(subscription) }); }
  async upload(file: Blob, name: string | null = null): Promise<ResourceRef> {
    if (file.size > 32 * 1024 * 1024) throw new BridgeError('message-too-large', 'not-started');
    const upload = handle(await this.request('user-resources.begin-upload', { mediaType: file.type || 'application/octet-stream', name, expectedSize: BigInt(file.size).toString() }));
    try {
      for (let offset = 0; offset < file.size; offset += 32768) await this.request('user-resources.append-upload', { handle: upload, bytes: encodeBytes(new Uint8Array(await file.slice(offset, offset + 32768).arrayBuffer())) });
      return await this.request('user-resources.finish-upload', { handle: upload }) as ResourceRef;
    } catch (error) { void this.request('user-resources.cancel-upload', { handle: upload }).catch(() => {}); throw error; }
  }
  async image(reference: AssetRef): Promise<{ url: string; revoke: () => void }> {
    if (!['image/png', 'image/jpeg', 'image/webp', 'image/gif', 'image/avif'].includes(reference.mediaType)) throw new BridgeError('invalid-asset', 'not-started');
    const reader = handle(await this.request('asset-store.open-asset', { reference })); const parts: Uint8Array<ArrayBuffer>[] = [];
    try { for (;;) { const bytes = decodeBytes(await this.request('asset-store.read-asset', { handle: reader, maximumBytes: 32768 })); if (!bytes.length) break; parts.push(bytes); } }
    finally { await this.request('asset-store.close-asset', { handle: reader }); }
    if (this.#closed) throw new BridgeError('channel-closed');
    const url = URL.createObjectURL(new Blob(parts, { type: reference.mediaType })); this.#urls.add(url);
    return { url, revoke: () => { URL.revokeObjectURL(url); this.#urls.delete(url); } };
  }
  dispose(): void { if (this.#slots.size) void this.#call('ui.slot.clear', {}).catch(() => {}); for (const cleanup of this.#slots.values()) cleanup(); this.#slots.clear(); this.#policyListeners.clear(); this.#visibilityListeners.clear(); this.#closed = true; this.#stop.abort(); for (const url of this.#urls) URL.revokeObjectURL(url); this.#urls.clear(); }
}

function forgetSession(): void { try { sessionStorage.removeItem('rintawa.session'); } catch { /* Server rejects/revokes the credential independently. */ } }

const tokens = {
  light: { background: '#ffffff', surface: '#f6f6f7', text: '#171719', muted: '#6d6d76', accent: '#765383', border: '#e6e6e9', hover: '#ededf0', raised: '#ffffff', error: '#be4545', positive: '#30724d', 'on-accent': '#ffffff', 'accent-hover': '#674574', 'accent-surface': '#f0eaf4', selected: '#e9dff0', 'selected-text': '#382044', 'border-strong': '#b3acba', placeholder: '#706a77', selection: '#765383', 'selection-text': '#ffffff', 'error-surface': '#fff0f0', overlay: '#0006', shadow: '#0003' },
  dark: { background: '#000000', surface: '#080808', text: '#eeeeef', muted: '#85858f', accent: '#c9a6db', border: '#1b1b1f', hover: '#131315', raised: '#101012', error: '#e58a8a', positive: '#8cbaa2', 'on-accent': '#170f1c', 'accent-hover': '#dfc1ed', 'accent-surface': '#1b1421', selected: '#211927', 'selected-text': '#eadcf2', 'border-strong': '#514758', placeholder: '#85808e', selection: '#634871', 'selection-text': '#ffffff', 'error-surface': '#190d10', overlay: '#000b', shadow: '#0009' },
};
export type Palette = typeof tokens.light;
export function applyTheme(theme: Theme, element: HTMLElement = document.documentElement): Palette {
  const palette = tokens[theme.mode]; element.style.colorScheme = theme.mode;
  for (const [key, value] of Object.entries(palette)) element.style.setProperty(`--rtw-${key}`, value);
  return palette;
}
export async function connectPanel(onTheme: (theme: Theme, palette: Palette) => void = () => {}): Promise<Client> {
  if (window.parent === window) throw new BridgeError('channel-closed', 'not-started');
  const params = new URLSearchParams(location.hash.slice(1)); const nonce = params.get('rtwBridge');
  if (!nonce || !/^[A-Za-z0-9_-]{43}$/.test(nonce)) throw new BridgeError('protocol-error');
  const origin = new URL(location.href).origin;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { window.removeEventListener('message', receive); reject(new BridgeError('channel-closed')); }, 5000);
    const receive = (event: MessageEvent) => {
      const data = event.data;
      if (event.source !== window.parent || event.origin !== origin || !record(data) || data.v !== 1 || data.type !== 'rtw:init' || data.nonce !== nonce || event.ports.length !== 1) return;
      clearTimeout(timer); window.removeEventListener('message', receive);
      const port = event.ports[0]; const rpc = new Rpc(value => port.postMessage(value), 2000, () => port.close()); const client = new Client((method, args) => rpc.request(method, args), viewState(data.state) ? data.state : {});
      client.visibilityChanged(data.visible !== false);
      const dispose = () => { client.dispose(); rpc.close(); port.close(); };
      port.onmessage = event => {
        const value = event.data;
        if (record(value) && value.v === 1 && value.type === 'theme' && record(value.theme) && (value.theme.mode === 'light' || value.theme.mode === 'dark')) {
          const theme = value.theme as Theme; onTheme(theme, applyTheme(theme));
        } else if (record(value) && value.v === 1 && value.type === 'visibility' && typeof value.visible === 'boolean') client.visibilityChanged(value.visible);
        else if (record(value) && value.v === 1 && value.type === 'permissions-changed') client.permissionsChanged();
        else if (record(value) && value.v === 1 && value.type === 'slot-closed' && typeof value.slot === 'string') client.slotClosed(value.slot);
        else if (record(value) && value.v === 1 && value.type === 'close') dispose();
        else rpc.accept(value);
      };
      port.onmessageerror = dispose; window.addEventListener('pagehide', dispose, { once: true }); port.start(); resolve(client);
    };
    window.addEventListener('message', receive);
    const hello = () => window.parent.postMessage({ v: 1, type: 'rtw:hello', nonce, visibility: true }, origin);
    if (document.readyState === 'complete') hello(); else window.addEventListener('load', hello, { once: true });
  });
}

type Rectangle = { x: number; y: number; width: number; height: number };
type Slot = { rect: Rectangle; clip?: Rectangle; box: HTMLDivElement; abort: AbortController; close?: () => Promise<void>; frame?: HTMLIFrameElement };
type FrameBinding = { view: View; state: ViewState; frame: HTMLIFrameElement; channel: string; nonce: string; parent?: FrameBinding; slots: Map<string, Slot>; bootstrap?: ReturnType<typeof setTimeout>; port?: MessagePort; loaded: boolean; used: boolean; closed: boolean; pending: Set<string>; visible?: boolean; visibilityEvents?: boolean; onClosed: () => void };
export class RootBridge {
  #socket: WebSocket;
  #rpc: Rpc;
  #channel: string;
  #frames = new Set<FrameBinding>();
  #visibilityTimer?: ReturnType<typeof setTimeout>;
  #visibilityWatch = () => {
    clearTimeout(this.#visibilityTimer); this.#visibilityTimer = undefined;
    if (this.#closed) return;
    for (const binding of this.#frames) {
      const visible = this.#frameVisible(binding);
      if (binding.visible !== visible) { binding.visible = visible; if (binding.visibilityEvents) binding.port?.postMessage({ v: 1, type: 'visibility', visible }); }
    }
    if (this.#frames.size && typeof document !== 'undefined') this.#visibilityTimer = setTimeout(this.#visibilityWatch, 250);
  };
  #frameVisible(binding: FrameBinding): boolean {
    if (typeof document === 'undefined' || typeof getComputedStyle !== 'function') return true;
    if (document.visibilityState === 'hidden' || !binding.frame.getClientRects().length) return false;
    const rect = binding.frame.getBoundingClientRect();
    if (!rect.width || !rect.height || rect.right <= 0 || rect.bottom <= 0 || rect.left >= innerWidth || rect.top >= innerHeight) return false;
    for (let element: HTMLElement | null = binding.frame; element; element = element.parentElement) {
      const style = getComputedStyle(element);
      if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') return false;
    }
    return !binding.parent || this.#frameVisible(binding.parent);
  }
  #theme: Theme = { mode: 'dark' };
  #closed = false;
  #reconcileTimer?: ReturnType<typeof setTimeout>;
  #disconnectListeners = new Set<() => void>();
  #receive: (event: MessageEvent) => void;
  #navigation: ((view: View, state: ViewState, source: View) => void) | null = null;
  onNavigate(handler: (view: View, state: ViewState, source: View) => void): () => void { this.#navigation = handler; return () => { if (this.#navigation === handler) this.#navigation = null; }; }
  #permissionRequest: ((view: View, request: PermissionRequest) => boolean) | null = null;
  onPermissionRequest(handler: (view: View, request: PermissionRequest) => boolean): () => void { this.#permissionRequest = handler; return () => { if (this.#permissionRequest === handler) this.#permissionRequest = null; }; }
  notifyPermissionsChanged(viewId: string): void { for (const b of this.#frames) if (!b.closed && b.view.id === viewId) b.port?.postMessage({ v: 1, type: 'permissions-changed' }); }
  #intent: ((method: string, args: Record<string, unknown>, source: View) => Promise<unknown>) | null = null;
  onIntent(handler: (method: string, args: Record<string, unknown>, source: View) => Promise<unknown>): () => void { this.#intent = handler; return () => { if (this.#intent === handler) this.#intent = null; }; }
  #secretEnded: ((reference:string)=>void) | null = null;
  onSecretPromptEnded(handler:(reference:string)=>void):()=>void {this.#secretEnded=handler;return()=>{if(this.#secretEnded===handler)this.#secretEnded=null;};}
  #secretHandler: ((reference: string) => boolean) | null = null;
  #secretQueue: string[] = [];
  onSecretPrompt(handler: (reference: string) => boolean): () => void {
    this.#secretHandler = handler;
    for (const reference of this.#secretQueue.splice(0)) this.#secretNotification(reference);
    return () => { if (this.#secretHandler === handler) this.#secretHandler = null; };
  }
  #secretNotification(reference: string): void {
    if (!this.#secretHandler && this.#secretQueue.length < 16) { this.#secretQueue.push(reference); return; }
    if (!this.#secretHandler?.(reference)) void this.client.request('secret-prompt.dismiss', {reference}).catch(()=>{});
  }
  async secretOperation(request: Record<string, unknown>, signal?: AbortSignal): Promise<unknown> {
    if(new TextEncoder().encode(JSON.stringify(request)).length>60*1024)throw new BridgeError('message-too-large','not-started');
    const readOnly = ['list','consumers','inspect-prompt'].includes(String(request.operation));
    for(let attempt=0;;attempt++) {
      try {return await this.#secretOnce(request,signal);}
      catch(failure) {if(!readOnly||attempt>=9||!(failure instanceof BridgeError)||failure.code!=='unavailable'||signal?.aborted)throw failure;await delay(100);}
    }
  }
  async #secretOnce(request: Record<string, unknown>, signal?: AbortSignal): Promise<unknown> {
    if(signal?.aborted)throw new BridgeError('channel-closed');
    const handle = await this.client.request('secret-administration.begin', request) as string;
    try {
      const deadline = Date.now()+120000;
      while (Date.now()<deadline) {
        if(signal?.aborted)throw new BridgeError('channel-closed');
        let status: {status:string;result?:unknown;code?:string};
        try {status = await this.client.request('secret-administration.status', {handle}) as typeof status;}
        catch(failure) {if(failure instanceof BridgeError && failure.code==='unavailable'){await delay(100);continue;}throw failure;}
        if (status.status==='completed') return status.result;
        if (status.status==='failed') throw new BridgeError(status.code ?? 'unavailable');
        await delay(100);
      }
      throw new BridgeError('outcome-unknown');
    } finally { await this.client.request('secret-administration.release', {handle}).catch(()=>{}); }
  }
  readonly client: Client;
  private constructor(socket: WebSocket, rpc: Rpc, channel: string) {
    this.#socket = socket; this.#rpc = rpc; this.#channel = channel;
    this.client = new Client((method, args) => rpc.request(method, args, channel));
    this.#receive = event => this.#hello(event); window.addEventListener('message', this.#receive);
    if (typeof document !== 'undefined') document.addEventListener('visibilitychange', this.#visibilityWatch);
    socket.addEventListener('close', () => {
      window.removeEventListener('message', this.#receive); rpc.close(); this.client.dispose();
      for (const binding of this.#frames) this.#detach(binding); this.#frames.clear();
      this.#closed = true; clearTimeout(this.#visibilityTimer); if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', this.#visibilityWatch);
      for (const listener of this.#disconnectListeners) listener();
    });
  }
  onDisconnect(listener: () => void): () => void {
    this.#disconnectListeners.add(listener);
    if (this.#closed) listener();
    return () => { this.#disconnectListeners.delete(listener); };
  }
  static async connect(): Promise<RootBridge> {
    if (window.top !== window || location.protocol !== 'http:' || location.hostname !== '127.0.0.1') throw new BridgeError('channel-closed', 'not-started');
    const bootstrap = location.hash.slice(1); history.replaceState(null, '', location.pathname + location.search);
    let token: string | null;
    try { token = bootstrap || sessionStorage.getItem('rintawa.session'); } catch { throw new BridgeError('unavailable', 'not-started'); }
    if (!token) throw new BridgeError('channel-closed', 'not-started');
    const socket = new WebSocket(`ws://${location.host}/bridge`); const rpc = new Rpc(value => socket.send(JSON.stringify(value)), 2000, () => socket.close());
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { forgetSession(); socket.close(); reject(new BridgeError('unavailable')); }, 5000);
      const fail = () => { clearTimeout(timer); forgetSession(); rpc.close(); socket.close(); reject(new BridgeError('channel-closed')); };
      socket.addEventListener('open', () => socket.send(JSON.stringify({ v: 1, type: 'auth', token })), { once: true });
      socket.addEventListener('error', fail, { once: true });
      const closed = () => fail(); socket.addEventListener('close', closed, { once: true });
      const ready = (event: MessageEvent) => {
        let value: unknown; try { value = JSON.parse(event.data); } catch { fail(); return; }
        if (!record(value) || value.v !== 1 || value.type !== 'ready' || typeof value.token !== 'string' || !asciiId(value.channel)) { fail(); return; }
        clearTimeout(timer); socket.removeEventListener('message', ready); socket.removeEventListener('close', closed); socket.removeEventListener('error', fail);
        try { sessionStorage.setItem('rintawa.session', value.token); } catch { rpc.close(); socket.close(); reject(new BridgeError('unavailable')); return; }
        const bridge = new RootBridge(socket, rpc, value.channel);
        socket.addEventListener('message', event => { try {
          const message = JSON.parse(event.data);
          if (record(message) && message.v === 1 && message.type === 'secret-prompt' && message.channel === bridge.#channel && asciiId(message.reference)) bridge.#secretNotification(message.reference);
          else if(record(message)&&message.v===1&&message.type==='secret-prompt-closed'&&message.channel===bridge.#channel&&asciiId(message.reference))bridge.#secretEnded?.(message.reference);
          else rpc.accept(message);
        } catch { rpc.close('protocol-error'); socket.close(); } });
        resolve(bridge);
      };
      socket.addEventListener('message', ready);
    });
  }
  async list(): Promise<View[]> {
    const views = await this.client.request('ui.list') as View[];
    for (const binding of this.#frames) if (!views.some(v => v.id === binding.view.id && v.activation === binding.view.activation)) void this.#release(binding).catch(() => this.disconnect());
    return views;
  }
  async mount(id: string, container: HTMLElement, onClosed: () => void = () => {}, state: ViewState = {}, signal?: AbortSignal): Promise<{ frame: HTMLIFrameElement; close: () => Promise<void> }> {
    const candidates = (await this.list()).filter(view => view.id === id);
    if (candidates.length !== 1 || !viewState(state)) throw new BridgeError('unavailable', 'not-started');
    const view = candidates[0];
    if (signal?.aborted) throw new DOMException('View mount cancelled', 'AbortError');
    let mounted: { channel: string; url: string };
    try { mounted = await this.client.request('ui.mount', { id }) as { channel: string; url: string }; }
    catch (error) { if (!(error instanceof BridgeError) || error.outcome === 'unknown') this.disconnect(); throw error; }
    try {
      if (signal?.aborted) throw new DOMException('View mount cancelled', 'AbortError');
      const target = new URL(mounted.url, location.origin);
      if (target.origin !== location.origin || !target.pathname.startsWith('/bundle/') || !asciiId(mounted.channel)) throw new BridgeError('protocol-error');
      const bytes = crypto.getRandomValues(new Uint8Array(32)); const nonce = encodeBytes(bytes).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
      target.hash = new URLSearchParams({ rtwBridge: nonce }).toString();
      const frame = document.createElement('iframe'); frame.sandbox.add('allow-scripts'); frame.referrerPolicy = 'no-referrer'; frame.src = target.href;
      const binding: FrameBinding = { view, state, frame, channel: mounted.channel, nonce, slots: new Map(), loaded: false, used: false, closed: false, pending: new Set(), onClosed };
      binding.bootstrap = setTimeout(() => { if (!binding.used) void this.#release(binding).catch(() => this.disconnect()); }, 5000);
      this.#frames.add(binding); this.#visibilityWatch(); frame.addEventListener('load', () => { if (binding.loaded) void this.#release(binding).catch(() => this.disconnect()); else binding.loaded = true; });
      container.append(frame);
      return { frame, close: () => this.#release(binding) };
    } catch (error) {
      try { await this.#unmountChannel(mounted.channel); } catch { this.disconnect(); }
      throw error;
    }
  }
  #hello(event: MessageEvent): void {
    const data = event.data;
    if (event.origin !== 'null' || !record(data) || data.v !== 1 || data.type !== 'rtw:hello' || typeof data.nonce !== 'string') return;
    const binding = [...this.#frames].find(binding => binding.loaded && !binding.used && !binding.closed && binding.frame.contentWindow === event.source && binding.nonce === data.nonce);
    if (!binding || !binding.frame.contentWindow) return;
    binding.used = true; binding.visibilityEvents = data.visibility === true; clearTimeout(binding.bootstrap); const ports = new MessageChannel(); binding.port = ports.port1;
    ports.port1.onmessage = event => {
      const value = event.data;
      if (binding.closed) return;
      if (!record(value) || value.v !== 1 || !asciiId(value.id) || typeof value.method !== 'string' || !record(value.args) || Object.keys(value).some(key => !['v', 'id', 'method', 'args'].includes(key)) || binding.pending.has(value.id)) { void this.#release(binding).catch(() => this.disconnect()); return; }
      const id = value.id;
      if (binding.pending.size >= 8) { ports.port1.postMessage({ v: 1, id, ok: false, error: { code: 'limit-exceeded', outcome: 'not-started' } }); return; }
      binding.pending.add(id);
      const operation = this.#callFrame(binding, value.method, value.args);
      operation.then(
        value => { if (!binding.closed) ports.port1.postMessage({ v: 1, id, ok: true, value }); },
        error => { if (!binding.closed) ports.port1.postMessage({ v: 1, id, ok: false, error: { code: error instanceof BridgeError ? error.code : 'unavailable', outcome: error instanceof BridgeError ? error.outcome : 'unknown' } }); },
      ).finally(() => binding.pending.delete(id));
    };
    ports.port1.onmessageerror = () => { void this.#release(binding).catch(() => this.disconnect()); }; ports.port1.start();
    binding.frame.contentWindow.postMessage({ v: 1, type: 'rtw:init', nonce: binding.nonce, state: binding.state, visible: this.#frameVisible(binding) }, '*', [ports.port2]);
    ports.port1.postMessage({ v: 1, type: 'theme', theme: this.#theme });
  }
  async #callFrame(binding: FrameBinding, method: string, args: Record<string, unknown>): Promise<unknown> {
    // Releasing a bound child is idempotent and needs no fresh catalog authority.
    if (method === 'ui.slot.close' || method === 'ui.slot.clear') return this.#slot(binding, method, args, []);
    if (binding.parent) { await this.list(); if (binding.closed) throw new BridgeError('channel-closed', 'not-started'); }
    if (method === 'ui.navigate') return this.#navigate(binding, args);
    if (method === 'ui.request-permissions') return this.#requestPermissions(binding, args);
    if (method === 'ui.views' || method.startsWith('ui.slot.') || ['ui.open-world', 'ui.create-world', 'ui.theme.set'].includes(method)) {
      const catalog = await this.list();
      const source = catalog.find(v => v.id === binding.view.id && v.activation === binding.view.activation);
      if (binding.closed || !source) throw new BridgeError('unavailable', 'not-started');
      if (method === 'ui.views') {
        if (Object.keys(args).length) throw new BridgeError('invalid-request', 'not-started');
        return catalog.filter(v => v.worldId === source.worldId);
      }
      if (method.startsWith('ui.slot.')) return this.#slot(binding, method, args, catalog);
      if (!this.#intent) throw new BridgeError('unavailable', 'not-started');
      return this.#intent(method, args, source);
    }
    return this.#rpc.request(method, args, binding.channel);
  }
  async #slot(binding: FrameBinding, method: string, args: Record<string, unknown>, catalog: View[]): Promise<null> {
    if (method === 'ui.slot.clear' && !Object.keys(args).length) { await Promise.all([...binding.slots.keys()].map(id => this.#closeSlot(binding, id))); return null; }
    const allowed = method === 'ui.slot.mount' ? ['slot', 'reference', 'rect', 'clip', 'state'] : method === 'ui.slot.update' ? ['slot', 'rect', 'clip'] : method === 'ui.slot.close' ? ['slot'] : [];
    if (!allowed.length || Object.keys(args).some(key => !allowed.includes(key)) || !asciiId(args.slot)) throw new BridgeError('invalid-request', 'not-started');
    const id = args.slot;
    if (method === 'ui.slot.close') { await this.#closeSlot(binding, id); return null; }
    const validRectangle = (value: unknown): value is Rectangle => record(value) && Object.keys(value).sort().join(',') === 'height,width,x,y' && Object.values(value).every(v => typeof v === 'number' && Number.isFinite(v) && Math.abs(v) <= 65536) && (value.width as number) >= 0 && (value.height as number) >= 0;
    if (!validRectangle(args.rect) || ('clip' in args && !validRectangle(args.clip))) throw new BridgeError('invalid-request', 'not-started');
    const geometry = args.rect, innerClip = args.clip as Rectangle | undefined;
    if (method === 'ui.slot.update') {
      const slot = binding.slots.get(id); if (!slot) throw new BridgeError('invalid-request', 'not-started');
      slot.rect = geometry; slot.clip = innerClip; this.#positionSlot(binding, slot); return null;
    }
    if (binding.slots.has(id) || !record(args.reference) || Object.keys(args.reference).sort().join(',') !== 'activation,componentId,instanceId' || !viewState(args.state)) throw new BridgeError('invalid-request', 'not-started');
    const target = catalog.filter(v => v.worldId === binding.view.worldId && v.restoreKey.instanceId === (args.reference as Record<string, unknown>).instanceId && v.restoreKey.componentId === (args.reference as Record<string, unknown>).componentId && v.activation === (args.reference as Record<string, unknown>).activation);
    if (target.length !== 1) throw new BridgeError('unavailable', 'not-started');
    const box = document.createElement('div'); box.className = 'rtw-view-slot'; box.style.cssText = 'position:fixed;overflow:hidden;display:none;isolation:isolate;';
    // A Root-owned sibling keeps the consuming extension away from the child's
    // bootstrap nonce, native channel and MessagePort. Consent stays in its own
    // later top-layer dialog; the slot is clipped to its parent's viewport.
    (binding.frame.closest('dialog') ?? document.body).append(box);
    const slot: Slot = { rect: geometry, clip: innerClip, box, abort: new AbortController() }; binding.slots.set(id, slot);
    this.#scheduleReconcile();
    try {
      const mounted = await this.mount(target[0].id, box, () => { if (binding.slots.get(id) === slot) { binding.slots.delete(id); box.remove(); binding.port?.postMessage({ v: 1, type: 'slot-closed', slot: id }); } }, args.state, slot.abort.signal);
      if (binding.closed || binding.slots.get(id) !== slot) { await mounted.close(); throw new BridgeError('channel-closed'); }
      slot.close = mounted.close; slot.frame = mounted.frame;
      const child = [...this.#frames].find(b => b.frame === mounted.frame)!; child.parent = binding;
      const update = () => { if (binding.closed || binding.slots.get(id) !== slot) return; this.#positionSlot(binding, slot); requestAnimationFrame(update); }; update();
      return null;
    } catch (error) { if (binding.slots.get(id) === slot) binding.slots.delete(id); box.remove(); throw error; }
  }
  #positionSlot(binding: FrameBinding, slot: Slot): void {
    const parent = binding.frame.getBoundingClientRect(), rect = slot.rect;
    const intended = { left: parent.left + rect.x, top: parent.top + rect.y };
    const clip = binding.parent ? binding.frame.parentElement!.getBoundingClientRect() : parent;
    let left = Math.max(0, parent.left, clip.left, intended.left), top = Math.max(0, parent.top, clip.top, intended.top);
    let right = Math.min(innerWidth, parent.right, clip.right, intended.left + rect.width), bottom = Math.min(innerHeight, parent.bottom, clip.bottom, intended.top + rect.height), visible = !!binding.frame.getClientRects().length;
    if (slot.clip) {
      left = Math.max(left, parent.left + slot.clip.x); top = Math.max(top, parent.top + slot.clip.y);
      right = Math.min(right, parent.left + slot.clip.x + slot.clip.width); bottom = Math.min(bottom, parent.top + slot.clip.y + slot.clip.height);
    }
    for (let element: HTMLElement | null = binding.frame; element; element = element.parentElement) {
      const style = getComputedStyle(element);
      if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') visible = false;
      const bounds = element.getBoundingClientRect();
      if (/^(hidden|clip|auto|scroll)$/.test(style.overflowX)) { left = Math.max(left, bounds.left + element.clientLeft); right = Math.min(right, bounds.left + element.clientLeft + element.clientWidth); }
      if (/^(hidden|clip|auto|scroll)$/.test(style.overflowY)) { top = Math.max(top, bounds.top + element.clientTop); bottom = Math.min(bottom, bounds.top + element.clientTop + element.clientHeight); }
    }
    const width = Math.max(0, right - left), height = Math.max(0, bottom - top);
    Object.assign(slot.box.style, { display: width && height && visible ? 'block' : 'none', left: `${left}px`, top: `${top}px`, width: `${width}px`, height: `${height}px` });
    if (slot.frame) Object.assign(slot.frame.style, { position: 'absolute', border: '0', left: `${intended.left - left}px`, top: `${intended.top - top}px`, width: `${rect.width}px`, height: `${rect.height}px` });
  }
  async #closeSlot(binding: FrameBinding, id: string): Promise<void> {
    const slot = binding.slots.get(id); if (!slot) return;
    binding.slots.delete(id); slot.abort.abort(); slot.box.remove(); await slot.close?.();
  }
  #scheduleReconcile(): void {
    if (this.#closed || this.#reconcileTimer || ![...this.#frames].some(binding => binding.slots.size)) return;
    this.#reconcileTimer = setTimeout(async () => {
      this.#reconcileTimer = undefined;
      try { await this.list(); } catch { /* Retry discovery; never replay a mutation. */ }
      this.#scheduleReconcile();
    }, 500);
  }
  async #requestPermissions(binding: FrameBinding, args: Record<string, unknown>): Promise<null> {
    if (!this.#permissionRequest || Object.keys(args).some(key => !['componentId', 'runtime', 'httpOrigins', 'secretRead'].includes(key)) || typeof args.componentId !== 'string' || !args.componentId || args.componentId.length > 128) throw new BridgeError('invalid-request', 'not-started');
    const runtime = args.runtime ?? [], httpOrigins = args.httpOrigins ?? [], secretRead = args.secretRead ?? [];
    for (const list of [runtime, httpOrigins, secretRead]) if (!Array.isArray(list) || list.length > 64 || list.some(v => typeof v !== 'string' || !v || v.length > 2048) || new Set(list).size !== list.length) throw new BridgeError('invalid-request', 'not-started');
    const request = { componentId: args.componentId, runtime, httpOrigins, secretRead } as PermissionRequest;
    if (request.runtime.length + request.httpOrigins.length + request.secretRead.length > 64) throw new BridgeError('limit-exceeded', 'not-started');
    for (const value of request.httpOrigins) {
      let origin: URL; try { origin = new URL(value); } catch { throw new BridgeError('invalid-request', 'not-started'); }
      if (origin.protocol !== 'http:' || origin.origin !== value || !(origin.hostname === 'localhost' || origin.hostname === '[::1]' || /^127\.\d+\.\d+\.\d+$/.test(origin.hostname))) throw new BridgeError('invalid-request', 'not-started');
    }
    const view = resolveView(binding.view.restoreKey, await this.list());
    if (binding.closed || !view || view.id !== binding.view.id) throw new BridgeError('unavailable', 'not-started');
    if (!this.#permissionRequest(view, request)) throw new BridgeError('limit-exceeded', 'not-started');
    return null;
  }
  async #navigate(binding: FrameBinding, args: Record<string, unknown>): Promise<null> {
    if (Object.keys(args).some(key => !['instanceId', 'componentId', 'activation', 'state'].includes(key)) || typeof args.instanceId !== 'string' || typeof args.componentId !== 'string' || !viewState(args.state) || !binding.view.worldId || !this.#navigation) throw new BridgeError('invalid-request', 'not-started');
    const catalog = await this.list();
    const source = resolveView(binding.view.restoreKey, catalog);
    const target = resolveView({ worldId: binding.view.worldId, instanceId: args.instanceId, componentId: args.componentId }, catalog);
    if (binding.closed || !source || source.id !== binding.view.id || !target || (args.activation !== undefined && args.activation !== target.activation)) throw new BridgeError('unavailable', 'not-started');
    const handler = this.#navigation; const state = args.state;
    // Reply before navigation retires the source port. Never transfer its authority.
    setTimeout(() => { if (!binding.closed && this.#navigation === handler) handler(target, state, source); }, 0);
    return null;
  }
  #detach(binding: FrameBinding): void {
    binding.closed = true;
    clearTimeout(binding.bootstrap);
    for (const id of [...binding.slots.keys()]) void this.#closeSlot(binding, id).catch(() => this.disconnect());
    for (const child of this.#frames) if (child.parent === binding) void this.#release(child).catch(() => this.disconnect());
    binding.port?.postMessage({ v: 1, type: 'close' }); binding.port?.close(); binding.frame.remove(); this.#frames.delete(binding); binding.onClosed();
  }
  async #release(binding: FrameBinding): Promise<void> {
    if (binding.closed) return; this.#detach(binding);
    await this.#unmountChannel(binding.channel);
  }
  async #unmountChannel(channel: string): Promise<void> {
    // Cleanup is idempotent; retrying it never retries a domain mutation.
    for (let attempt = 0; ; attempt++) {
      try { await this.client.request('ui.unmount', { channel }); return; }
      catch (error) {
        if (error instanceof BridgeError && ['channel-closed', 'invalid-request'].includes(error.code)) return;
        if (error instanceof BridgeError && ['limit-exceeded', 'unavailable'].includes(error.code) && attempt < 40 && this.#socket.readyState === WebSocket.OPEN) { await delay(25); continue; }
        throw error;
      }
    }
  }
  theme(theme: Theme): void { this.#theme = theme; applyTheme(theme); for (const binding of this.#frames) binding.port?.postMessage({ v: 1, type: 'theme', theme }); }
  async getLayout(): Promise<unknown> { return this.client.request('ui.layout.get'); }
  async setLayout(layout: unknown): Promise<unknown> { return this.client.request('ui.layout.set', { layout }); }
  async resetLayout(): Promise<unknown> { return this.client.request('ui.layout.reset'); }
  async logout(): Promise<void> { try { await this.client.request('session.close'); } finally { forgetSession(); this.disconnect(); } }
  disconnect(): void { this.#closed = true; clearTimeout(this.#visibilityTimer); if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', this.#visibilityWatch); clearTimeout(this.#reconcileTimer); window.removeEventListener('message', this.#receive); for (const binding of this.#frames) this.#detach(binding); this.#socket.close(); this.#rpc.close(); this.client.dispose(); }
}
