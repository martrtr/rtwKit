/** Chat's context descriptors are inert; Native enforces the invocation recipient. */
import type { Caller, Client } from '@rintawa/bridge';
import { DomainClient, DomainError } from '@rintawa/domain';
export type MessageAction = { key: string; id: string; label: string; order: number; point: string; provider: Caller };
export type MessageContext = { conversationId: string; messageId: string; expectedRevision: string };
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);
const id = (v: unknown): v is string => typeof v === 'string' && /^[\x21-\x7e]{1,64}$/.test(v);
const point = (v: unknown): v is string => typeof v === 'string' && v.length <= 256 && v.split('/').length <= 5 && v.split('/').every(s=>/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(s) && !s.includes('..'));
export async function messageActions(client: Client, signal?: AbortSignal): Promise<{ items: MessageAction[]; rejected: number }> {
  const replies = await client.querySourced('rintawa/chat/message-actions/v1', new TextEncoder().encode('{"method":"describe"}'), signal);
  const items: MessageAction[] = [], seen = new Set<string>(), duplicate = new Set<string>(); let rejected = 0;
  for (const reply of replies) {
    const owner = JSON.stringify([reply.provider.instanceId, reply.provider.componentId, reply.provider.activation]);
    if (seen.has(owner)) { duplicate.add(owner); rejected++; continue; } seen.add(owner);
    try {
      if (reply.payload.length > 65536) throw Error();
      const data: unknown = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(reply.payload));
      if (!object(data) || Object.keys(data).sort().join(',') !== 'actionPoint,actions' || !point(data.actionPoint) || !Array.isArray(data.actions) || data.actions.length > 16) throw Error();
      const local = new Set<string>(), next: MessageAction[] = [];
      for (const value of data.actions) {
        if (!object(value) || Object.keys(value).sort().join(',') !== 'id,label,order' || !id(value.id) || local.has(value.id) || typeof value.label !== 'string' || !value.label.trim() || new TextEncoder().encode(value.label).length > 128 || !Number.isInteger(value.order) || Math.abs(value.order as number) > 1000) throw Error();
        local.add(value.id); next.push({ key: `${owner}/${value.id}`, id: value.id, label: value.label, order: value.order as number, point: data.actionPoint, provider: { ...reply.provider } });
      }
      items.push(...next);
    } catch { rejected++; }
  }
  return { items: items.filter(item=>!duplicate.has(JSON.stringify([item.provider.instanceId,item.provider.componentId,item.provider.activation]))).sort((a,b)=>a.order-b.order||a.label.localeCompare(b.label)||a.key.localeCompare(b.key)), rejected };
}
export async function invokeMessageAction(client: Client, action: MessageAction, context: MessageContext, signal: AbortSignal): Promise<string> {
  const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
  if (!uuid.test(context.conversationId) || !uuid.test(context.messageId) || !/^(0|[1-9][0-9]{0,19})$/.test(context.expectedRevision) || BigInt(context.expectedRevision) > 18446744073709551615n) throw new DomainError('invalid-request','not-started');
  const domain = new DomainClient({ query: (point,payload,stop)=>client.queryTo(point,payload,action.provider,stop) }, action.point);
  let operation: string | undefined;
  const result = (value: unknown): { status: string; operation?: string; message?: string; reason?: string } => {
    if (!object(value) || !['pending','completed','failed'].includes(value.status as string) || Object.keys(value).some(k=>!['status','operation','message','reason'].includes(k))) throw new DomainError('protocol-error');
    if (value.status==='pending' && (typeof value.operation!=='string'||!/^[A-Za-z0-9_-]{43}$/.test(value.operation))) throw new DomainError('protocol-error');
    if (value.message!==undefined && (typeof value.message!=='string'||new TextEncoder().encode(value.message).length>1024)) throw new DomainError('protocol-error');
    if (value.status==='failed' && (typeof value.reason!=='string'||new TextEncoder().encode(value.reason).length>2048)) throw new DomainError('protocol-error');
    return value as { status:string;operation?:string;message?:string;reason?:string };
  };
  const deadline = performance.now()+30000;
  try {
    let value = result(await domain.request('invoke',{actionId:action.id,context},signal));
    while (value.status==='pending') {
      if (operation && value.operation !== operation) throw new DomainError('protocol-error');
      operation=value.operation;
      if (signal.aborted) throw new DOMException('Action observation closed','AbortError');
      if (performance.now()>=deadline) throw new DomainError('action-outcome-unknown');
      await new Promise(resolve=>setTimeout(resolve,80));
      value=result(await domain.request('status',{operation},signal));
    }
    if (value.status==='failed') throw new DomainError(value.reason!,'not-started');
    return value.message??'Action completed.';
  } finally {
    // Releasing a pending result does not cancel admitted provider work.
    if (operation) void domain.release({operation,itemIds:[]}).catch(()=>{});
  }
}
