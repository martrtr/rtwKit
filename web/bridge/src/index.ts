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
export type View = { id: string; title: string; worldId: string | null; restoreKey: RestoreKey };
export type Caller = { instanceId: string; componentId: string; activation: string };
export type InboundQuery = { handle: string; caller: Caller; payload: Uint8Array };
export type AssetRef = { digest: string; size: string; mediaType: string };
export type ResourceRef = { id: string; size: string; mediaType: string; name: string | null };
export type ViewState = Record<string, string>;
export function viewState(value: unknown): value is ViewState {
  return record(value) && Object.keys(value).length <= 8 && Object.entries(value).every(([key, value]) => asciiId(key) && typeof value === 'string' && new TextEncoder().encode(value).length <= 256) && new TextEncoder().encode(JSON.stringify(value)).length <= 1024;
}
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
  readonly initialState: ViewState;
  constructor(call: Call, initialState: ViewState = {}) { this.#call = call; this.initialState = initialState; }
  async getState(): Promise<ViewState> { const state = await this.request('ui.state.get'); if (!viewState(state)) throw new BridgeError('invalid-stored-data'); return state; }
  async setState(state: ViewState): Promise<void> { if (!viewState(state)) throw new BridgeError('invalid-request', 'not-started'); await this.request('ui.state.set', { state }); }
  async navigate(instanceId: string, componentId: string, state: ViewState = {}): Promise<void> { if (!viewState(state)) throw new BridgeError('invalid-request', 'not-started'); await this.request('ui.navigate', { instanceId, componentId, state }); }
  request(method: string, args: Record<string, unknown> = {}): Promise<unknown> {
    return this.#closed ? Promise.reject(new BridgeError('channel-closed', 'not-started')) : this.#call(method, args);
  }
  async beginQuery(point: string, payload: Uint8Array): Promise<string> {
    if (payload.length > 65536) throw new BridgeError('message-too-large', 'not-started');
    return handle(await this.request('transport.begin-query', { point, payload: encodeBytes(payload) }));
  }
  async pollQuery(query: string): Promise<Uint8Array[] | null> {
    const result = await this.request('transport.poll-query', { handle: handle(query) });
    if (result === null) return null;
    if (!Array.isArray(result)) throw new BridgeError('protocol-error');
    return result.map(decodeBytes);
  }
  closeQuery(query: string): Promise<unknown> { return this.request('transport.close-query', { handle: handle(query) }); }
  async query(point: string, payload: Uint8Array): Promise<Uint8Array[]> {
    const query = await this.beginQuery(point, payload);
    try {
      for (;;) {
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
  dispose(): void { this.#closed = true; this.#stop.abort(); for (const url of this.#urls) URL.revokeObjectURL(url); this.#urls.clear(); }
}

function forgetSession(): void { try { sessionStorage.removeItem('rintawa.session'); } catch { /* Server rejects/revokes the credential independently. */ } }

const tokens = {
  light: { background: '#ffffff', surface: '#f6f6f7', text: '#171719', muted: '#6d6d76', accent: '#765383', border: '#e6e6e9', hover: '#ededf0', raised: '#ffffff', error: '#be4545', positive: '#438462' },
  dark: { background: '#000000', surface: '#080808', text: '#eeeeef', muted: '#85858f', accent: '#c9a6db', border: '#1b1b1f', hover: '#131315', raised: '#101012', error: '#e58a8a', positive: '#8cbaa2' },
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
      const dispose = () => { client.dispose(); rpc.close(); port.close(); };
      port.onmessage = event => {
        const value = event.data;
        if (record(value) && value.v === 1 && value.type === 'theme' && record(value.theme) && (value.theme.mode === 'light' || value.theme.mode === 'dark')) {
          const theme = value.theme as Theme; onTheme(theme, applyTheme(theme));
        } else if (record(value) && value.v === 1 && value.type === 'close') dispose();
        else rpc.accept(value);
      };
      port.onmessageerror = dispose; window.addEventListener('pagehide', dispose, { once: true }); port.start(); resolve(client);
    };
    window.addEventListener('message', receive);
    const hello = () => window.parent.postMessage({ v: 1, type: 'rtw:hello', nonce }, origin);
    if (document.readyState === 'complete') hello(); else window.addEventListener('load', hello, { once: true });
  });
}

type FrameBinding = { view: View; state: ViewState; frame: HTMLIFrameElement; channel: string; nonce: string; port?: MessagePort; loaded: boolean; used: boolean; closed: boolean; pending: Set<string>; onClosed: () => void };
export class RootBridge {
  #socket: WebSocket;
  #rpc: Rpc;
  #channel: string;
  #frames = new Set<FrameBinding>();
  #theme: Theme = { mode: 'dark' };
  #closed = false;
  #disconnectListeners = new Set<() => void>();
  #receive: (event: MessageEvent) => void;
  #navigation: ((view: View, state: ViewState, source: View) => void) | null = null;
  onNavigate(handler: (view: View, state: ViewState, source: View) => void): () => void { this.#navigation = handler; return () => { if (this.#navigation === handler) this.#navigation = null; }; }
  readonly client: Client;
  private constructor(socket: WebSocket, rpc: Rpc, channel: string) {
    this.#socket = socket; this.#rpc = rpc; this.#channel = channel;
    this.client = new Client((method, args) => rpc.request(method, args, channel));
    this.#receive = event => this.#hello(event); window.addEventListener('message', this.#receive);
    socket.addEventListener('close', () => {
      window.removeEventListener('message', this.#receive); rpc.close(); this.client.dispose();
      for (const binding of this.#frames) this.#detach(binding); this.#frames.clear();
      this.#closed = true;
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
        socket.addEventListener('message', event => { try { rpc.accept(JSON.parse(event.data)); } catch { rpc.close('protocol-error'); socket.close(); } });
        resolve(new RootBridge(socket, rpc, value.channel));
      };
      socket.addEventListener('message', ready);
    });
  }
  async list(): Promise<View[]> { return await this.client.request('ui.list') as View[]; }
  async mount(id: string, container: HTMLElement, onClosed: () => void = () => {}, state: ViewState = {}, signal?: AbortSignal): Promise<{ frame: HTMLIFrameElement; close: () => Promise<void> }> {
    const candidates = (await this.list()).filter(view => view.id === id);
    if (candidates.length !== 1 || !viewState(state)) throw new BridgeError('unavailable', 'not-started');
    const view = candidates[0];
    if (signal?.aborted) throw new DOMException('View mount cancelled', 'AbortError');
    const mounted = await this.client.request('ui.mount', { id }) as { channel: string; url: string };
    try {
      if (signal?.aborted) throw new DOMException('View mount cancelled', 'AbortError');
      const target = new URL(mounted.url, location.origin);
      if (target.origin !== location.origin || !target.pathname.startsWith('/bundle/') || !asciiId(mounted.channel)) throw new BridgeError('protocol-error');
      const bytes = crypto.getRandomValues(new Uint8Array(32)); const nonce = encodeBytes(bytes).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
      target.hash = new URLSearchParams({ rtwBridge: nonce }).toString();
      const frame = document.createElement('iframe'); frame.sandbox.add('allow-scripts'); frame.referrerPolicy = 'no-referrer'; frame.src = target.href;
      const binding: FrameBinding = { view, state, frame, channel: mounted.channel, nonce, loaded: false, used: false, closed: false, pending: new Set(), onClosed };
      this.#frames.add(binding); frame.addEventListener('load', () => { if (binding.loaded) void this.#release(binding).catch(() => this.disconnect()); else binding.loaded = true; });
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
    binding.used = true; const ports = new MessageChannel(); binding.port = ports.port1;
    ports.port1.onmessage = event => {
      const value = event.data;
      if (binding.closed) return;
      if (!record(value) || value.v !== 1 || !asciiId(value.id) || typeof value.method !== 'string' || !record(value.args) || Object.keys(value).some(key => !['v', 'id', 'method', 'args'].includes(key)) || binding.pending.has(value.id)) { void this.#release(binding).catch(() => this.disconnect()); return; }
      const id = value.id;
      if (binding.pending.size >= 8) { ports.port1.postMessage({ v: 1, id, ok: false, error: { code: 'limit-exceeded', outcome: 'not-started' } }); return; }
      binding.pending.add(id);
      const operation = value.method === 'ui.navigate' ? this.#navigate(binding, value.args) : this.#rpc.request(value.method, value.args, binding.channel);
      operation.then(
        value => { if (!binding.closed) ports.port1.postMessage({ v: 1, id, ok: true, value }); },
        error => { if (!binding.closed) ports.port1.postMessage({ v: 1, id, ok: false, error: { code: error instanceof BridgeError ? error.code : 'unavailable', outcome: error instanceof BridgeError ? error.outcome : 'unknown' } }); },
      ).finally(() => binding.pending.delete(id));
    };
    ports.port1.onmessageerror = () => { void this.#release(binding).catch(() => this.disconnect()); }; ports.port1.start();
    binding.frame.contentWindow.postMessage({ v: 1, type: 'rtw:init', nonce: binding.nonce, state: binding.state }, '*', [ports.port2]);
    ports.port1.postMessage({ v: 1, type: 'theme', theme: this.#theme });
  }
  async #navigate(binding: FrameBinding, args: Record<string, unknown>): Promise<null> {
    if (Object.keys(args).some(key => !['instanceId', 'componentId', 'state'].includes(key)) || typeof args.instanceId !== 'string' || typeof args.componentId !== 'string' || !viewState(args.state) || !binding.view.worldId || !this.#navigation) throw new BridgeError('invalid-request', 'not-started');
    const catalog = await this.list();
    const source = resolveView(binding.view.restoreKey, catalog);
    const target = resolveView({ worldId: binding.view.worldId, instanceId: args.instanceId, componentId: args.componentId }, catalog);
    if (binding.closed || !source || source.id !== binding.view.id || !target) throw new BridgeError('unavailable', 'not-started');
    const handler = this.#navigation; const state = args.state;
    // Reply before navigation retires the source port. Never transfer its authority.
    setTimeout(() => { if (!binding.closed && this.#navigation === handler) handler(target, state, source); }, 0);
    return null;
  }
  #detach(binding: FrameBinding): void { binding.closed = true; binding.port?.postMessage({ v: 1, type: 'close' }); binding.port?.close(); binding.frame.remove(); this.#frames.delete(binding); binding.onClosed(); }
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
  disconnect(): void { window.removeEventListener('message', this.#receive); for (const binding of this.#frames) this.#detach(binding); this.#socket.close(); this.#rpc.close(); this.client.dispose(); }
}
