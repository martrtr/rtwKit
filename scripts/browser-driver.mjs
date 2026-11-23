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
    this.socket = socket; this.sequence = 0; this.pending = new Map(); this.contexts = new Map(); this.errors = []; this.worldCalls = []; this.worldCommands = []; this.resourceFailures = []; this.methods = new Map();
    socket.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      if (message.id) { const pending = this.pending.get(message.id); if (!pending) return; this.pending.delete(message.id); clearTimeout(pending.timer); message.error ? pending.reject(new Error(message.error.message)) : pending.resolve(message.result); }
      if (message.method === 'Network.webSocketFrameSent') { try { const v = JSON.parse(message.params.response.payloadData); if (v.method?.startsWith('world-sessions.')) { this.methods.set(`${message.params.requestId}:${v.id}`, v.method); this.worldCommands.push({method:v.method,args:v.args}); } } catch {} }
      if (message.method === 'Network.webSocketFrameReceived') { try { const v = JSON.parse(message.params.response.payloadData); const requestKey=`${message.params.requestId}:${v.id}`; const method = this.methods.get(requestKey); this.methods.delete(requestKey); if (method) this.worldCalls.push({ method, ok: v.ok, value: v.value, error: v.error }); } catch {} }
      if (message.method === 'Network.responseReceived' && message.params.response.status >= 400) this.resourceFailures.push({ url: redact(message.params.response.url), status: message.params.response.status, type: message.params.type });
      if (message.method === 'Runtime.executionContextCreated') this.contexts.set(`${message.sessionId ?? ''}:${message.params.context.id}`, { ...message.params.context, sessionId: message.sessionId });
      if (message.method === 'Runtime.executionContextDestroyed') this.contexts.delete(`${message.sessionId ?? ''}:${message.params.executionContextId}`);
      if (message.method === 'Runtime.executionContextsCleared' || message.method === 'Target.detachedFromTarget') for (const [key, context] of this.contexts) { if (context.sessionId === (message.params?.sessionId ?? message.sessionId)) this.contexts.delete(key); }
      if (message.method === 'Target.attachedToTarget') { const session = message.params.sessionId; void this.call('Runtime.enable', {}, session).catch(() => {}); void this.call('Log.enable', {}, session).catch(() => {}); void this.call('Network.enable', {}, session).catch(() => {}); }
      if (message.method === 'Runtime.exceptionThrown') this.errors.push(redact(message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text));
      if (message.method === 'Log.entryAdded' && message.params.entry.level === 'error') {
        this.errors.push(redact(message.params.entry.text));
        const entry=message.params.entry;const status=/status of (\d{3})/.exec(entry.text)?.[1];
        if(status&&entry.url&&!this.resourceFailures.some(failure=>failure.url===redact(entry.url)&&failure.status===Number(status)))this.resourceFailures.push({url:redact(entry.url),status:Number(status),type:'Log'});
      }
    });
  }
  static async connect(url) { const socket = new WebSocket(url); await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', () => reject(new Error('CDP connection failed')), { once: true }); }); return new CDP(socket); }
  call(method, params = {}, sessionId) { const id = ++this.sequence; return new Promise((resolve, reject) => { const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`CDP timeout: ${method}`)); }, 10000); this.pending.set(id, { resolve, reject, timer }); this.socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })); }); }
  async evaluate(expression, contextId, userGesture = false) { const result = await this.call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true, userGesture, ...(contextId ? { contextId: contextId.id } : {}) }, contextId?.sessionId); if (result.exceptionDetails) throw new Error(redact(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text)); return result.result.value; }
  async body(context) { return this.evaluate('document.body?.innerText ?? ""', context); }
  async click(text, context) { return eventually(() => this.evaluate(`(() => { const scope = [...document.querySelectorAll('dialog:modal')].at(-1) ?? document; const button = [...scope.querySelectorAll('button')].find(b => !b.disabled && b.getClientRects().length && getComputedStyle(b).visibility !== 'hidden' && (b.getAttribute('aria-label') === ${JSON.stringify(text)} || b.title === ${JSON.stringify(text)} || b.textContent.trim() === ${JSON.stringify(text)})); if (!button) return false; button.click(); return true; })()`, context, true), `visible enabled button ${text}`); }
  async openSettings() {
    if (await this.evaluate('[...document.querySelectorAll("dialog:modal h2")].some(h=>h.textContent==="Settings")')) return;
    if (!await this.evaluate('!!document.querySelector("#workspace-menu:popover-open")')) await this.click('Workspace controls');
    await this.click('Settings');
  }
  async openSettingsSection(title) {
    await this.openSettings();
    const renderer=await eventually(async()=>(await this.panelContexts('.settings-layout'))[0],'selected Settings renderer',30000);
    await this.waitText(title,renderer);await this.click(title,renderer);return renderer;
  }
  async waitShell() { return eventually(() => this.evaluate('!!document.querySelector(\'button[aria-label="Home"]\')'), 'shell ready'); }
  async setInput(selector, value, context) { return this.evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e)throw Error('input unavailable');const proto=e instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;Object.getOwnPropertyDescriptor(proto,'value').set.call(e,${JSON.stringify(value)});e.dispatchEvent(new Event('input',{bubbles:true}));})()`, context); }
  // Administrative empty-World fixture; story-onboarding-smoke exercises the actual chooser.
  async createEmptyWorld(name) {
    await this.evaluate(`(async()=>{const ws=new WebSocket('ws://'+location.host+'/bridge');let channel,n=0;const pending=new Map();await new Promise((resolve,reject)=>{ws.onopen=()=>ws.send(JSON.stringify({v:1,type:'auth',token:sessionStorage.getItem('rintawa.session')}));ws.onerror=()=>reject(Error('fixture auth failed'));ws.onmessage=e=>{const v=JSON.parse(e.data);if(v.type==='ready'){channel=v.channel;resolve();}else{const p=pending.get(v.id);if(p){pending.delete(v.id);clearTimeout(p.timer);v.ok?p.resolve(v.value):p.reject(Error(v.error.code));}}};});const rpc=(method,args={})=>new Promise((resolve,reject)=>{const id=String(++n);const timer=setTimeout(()=>reject(Error('fixture timeout')),5000);pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({v:1,id,channel,method,args}));});try{const world=await rpc('world-sessions.create');await rpc('world-sessions.set-metadata',{worldId:world.worldId,title:${JSON.stringify(name)},description:null,cover:null});}finally{ws.close();}})()`);
    await this.click('Home'); await this.click('Refresh worlds'); await this.openWorld(name); await this.waitText('Start World');
  }
  async openWorld(name) { await this.click(`Open world ${name}`); }
  async selectConversation(title, context) { if (await this.evaluate('document.querySelector(".chat")?.classList.contains("navigation-closed")', context)) await this.click('Toggle conversations', context); await this.click(title, context); }
  async openConversationDialog(context) { if (await this.evaluate('document.querySelector(".chat")?.classList.contains("navigation-closed")', context)) await this.click('Toggle conversations', context); await this.click('New conversation', context); await eventually(() => this.evaluate('!!document.querySelector(\'dialog:modal #new-title\')', context), 'new conversation dialog'); }

  async waitText(text, context) { return eventually(async () => (await this.body(context)).includes(text), `text ${text}`); }
  async panelContexts(selector = "canvas") { const result = []; for (const context of this.contexts.values()) if (context.auxData?.isDefault && context.auxData.frameId !== this.rootFrame) { try { if (await this.evaluate(`!!document.querySelector(${JSON.stringify(selector)})`, context)) result.push(context); } catch {} } return result; }
}
