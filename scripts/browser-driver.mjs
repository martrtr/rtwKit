// Local Chromium CDP driver shared by browser acceptance scripts.
export const pause = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
export const redact = value => value.replace(/#[A-Za-z0-9_-]{43}/g, '#<redacted>');
export async function eventually(action, label, timeout = 15000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) { const value = await action(); if (value) return value; await pause(60); }
  throw new Error(`Timed out: ${label}`);
}
export class CDP {
  constructor(socket) {
    this.socket = socket; this.sequence = 0; this.pending = new Map(); this.contexts = new Map(); this.errors = []; this.worldCalls = []; this.methods = new Map();
    socket.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      if (message.id) { const pending = this.pending.get(message.id); if (!pending) return; this.pending.delete(message.id); clearTimeout(pending.timer); message.error ? pending.reject(new Error(message.error.message)) : pending.resolve(message.result); }
      if (message.method === 'Network.webSocketFrameSent') { try { const v = JSON.parse(message.params.response.payloadData); if (v.method?.startsWith('world-sessions.')) this.methods.set(v.id, v.method); } catch {} }
      if (message.method === 'Network.webSocketFrameReceived') { try { const v = JSON.parse(message.params.response.payloadData); const method = this.methods.get(v.id); if (method) this.worldCalls.push({ method, ok: v.ok, value: v.value, error: v.error }); } catch {} }
      if (message.method === 'Runtime.executionContextCreated') this.contexts.set(`${message.sessionId ?? ''}:${message.params.context.id}`, { ...message.params.context, sessionId: message.sessionId });
      if (message.method === 'Runtime.executionContextDestroyed') this.contexts.delete(`${message.sessionId ?? ''}:${message.params.executionContextId}`);
      if (message.method === 'Runtime.executionContextsCleared' || message.method === 'Target.detachedFromTarget') for (const [key, context] of this.contexts) { if (context.sessionId === (message.params?.sessionId ?? message.sessionId)) this.contexts.delete(key); }
      if (message.method === 'Target.attachedToTarget') { const session = message.params.sessionId; void this.call('Runtime.enable', {}, session).catch(() => {}); void this.call('Log.enable', {}, session).catch(() => {}); }
      if (message.method === 'Runtime.exceptionThrown') this.errors.push(redact(message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text));
      if (message.method === 'Log.entryAdded' && message.params.entry.level === 'error') this.errors.push(redact(message.params.entry.text));
    });
  }
  static async connect(url) { const socket = new WebSocket(url); await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', () => reject(new Error('CDP connection failed')), { once: true }); }); return new CDP(socket); }
  call(method, params = {}, sessionId) { const id = ++this.sequence; return new Promise((resolve, reject) => { const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`CDP timeout: ${method}`)); }, 10000); this.pending.set(id, { resolve, reject, timer }); this.socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })); }); }
  async evaluate(expression, contextId) { const result = await this.call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true, ...(contextId ? { contextId: contextId.id } : {}) }, contextId?.sessionId); if (result.exceptionDetails) throw new Error(redact(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text)); return result.result.value; }
  async body(context) { return this.evaluate('document.body?.innerText ?? ""', context); }
  async click(text, context) { return eventually(() => this.evaluate(`(() => { const button = [...document.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)} && !b.disabled); if (!button) return false; button.click(); return true; })()`, context), `enabled button ${text}`); }
  async waitText(text, context) { return eventually(async () => (await this.body(context)).includes(text), `text ${text}`); }
  async panelContexts(selector = "canvas") { const result = []; for (const context of this.contexts.values()) if (context.auxData?.isDefault && context.auxData.frameId !== this.rootFrame) { try { if (await this.evaluate(`!!document.querySelector(${JSON.stringify(selector)})`, context)) result.push(context); } catch {} } return result; }
}
