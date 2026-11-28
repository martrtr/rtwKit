import type { Caller, Client } from '@rintawa/bridge';
export type Catalog = { id: string; label: string; order: number; provider: Caller; key: string };
export type Item = { itemId: string; label: string; detail: string };
const point = 'rintawa/scenes/catalogs/v1';
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const text = (v: unknown, n: number): v is string => typeof v === 'string' && new TextEncoder().encode(v).length <= n;
export const identity = (p: Caller) => JSON.stringify([p.instanceId, p.componentId, p.activation]);
function decode(bytes: Uint8Array) {
  if (bytes.length > 65536) throw Error('Catalog response too large.');
  const v = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  if (v?.ok !== true) throw Error(v?.error?.code ?? 'Catalog unavailable.');
  return v.value;
}
function items(v: unknown): Item[] {
  if (!Array.isArray(v) || v.length > 32) throw Error('Invalid catalog results.');
  const seen = new Set<string>();
  return v.map(row => {
    if (!uuid.test(row?.itemId) || seen.has(row.itemId) || !text(row.label, 256) || !row.label.trim() || !text(row.detail, 512)) throw Error('Invalid catalog item.');
    seen.add(row.itemId); return { itemId: row.itemId, label: row.label, detail: row.detail };
  });
}
export async function discover(client: Client, signal?: AbortSignal): Promise<{ items: Catalog[]; rejected: number }> {
  const replies = await client.querySourced(point, new TextEncoder().encode('{"method":"describe"}'), signal);
  const out: Catalog[] = [], seen = new Set<string>(), duplicates = new Set<string>(); let rejected = 0;
  for (const reply of replies) {
    const owner = identity(reply.provider);
    if (seen.has(owner)) { duplicates.add(owner); rejected++; continue; } seen.add(owner);
    try {
      const v = decode(reply.payload); if (!Array.isArray(v.catalogs) || v.catalogs.length > 8) throw Error();
      const local = new Set<string>(), next: Catalog[] = [];
      for (const c of v.catalogs) {
        if (typeof c.id !== 'string' || !/^[A-Za-z0-9._-]{1,64}$/.test(c.id) || local.has(c.id) || !text(c.label, 128) || !c.label.trim() || !Number.isInteger(c.order) || Math.abs(c.order) > 1000) throw Error();
        local.add(c.id); next.push({ id: c.id, label: c.label, order: c.order, key: owner + '/' + c.id, provider: reply.provider });
      } out.push(...next);
    } catch { rejected++; }
  }
  return { items: out.filter(c => !duplicates.has(identity(c.provider))).sort((a, b) => a.order - b.order || a.label.localeCompare(b.label) || a.key.localeCompare(b.key)), rejected };
}
export async function search(client: Client, catalog: Catalog, query: string, after: string | null, signal: AbortSignal): Promise<{ items: Item[]; next: string | null }> {
  if (!text(query, 256)) throw Error('Search is too long.');
  const replies = await client.querySourced(point, new TextEncoder().encode(JSON.stringify({ method: 'search', catalogId: catalog.id, query, after, provider: catalog.provider })), signal);
  const matches = replies.filter(r => identity(r.provider) === identity(catalog.provider));
  if (matches.length !== 1) throw Error('Selected catalog unavailable.');
  const v = decode(matches[0].payload);
  if (v.next !== null && (typeof v.next !== 'string' || !uuid.test(v.next) || v.next === after)) throw Error('Invalid catalog cursor.');
  return { items: items(v.items), next: v.next };
}
export async function labels(client: Client, ids: string[], signal: AbortSignal): Promise<Map<string, string>> {
  const replies = await client.querySourced(point, new TextEncoder().encode(JSON.stringify({ method: 'describe-items', itemIds: ids })), signal);
  const names = new Map<string, Set<string>>();
  for (const reply of replies) {
    try {
      const rows = items(decode(reply.payload).items); if (rows.some(r => !ids.includes(r.itemId))) continue;
      for (const r of rows) { let set = names.get(r.itemId); if (!set) { set = new Set(); names.set(r.itemId, set); } set.add(r.label); }
    } catch { /* A missing catalog does not hide existing member references. */ }
  }
  return new Map([...names].map(([id, set]) => [id, [...set].sort().join(' / ')]));
}
