import type { Client, View } from '@rintawa/bridge';
export type ViewContribution = { id: string; label: string; order: number; view: View };
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);
/** Consumers validate their own point. Only Native responder identity can select a view. */
export async function viewContributions(client: Client, point: string, labelKey: 'title' | 'label'): Promise<{ items: ViewContribution[]; rejected: number }> {
  const replies = await client.querySourced(point, new TextEncoder().encode('{"method":"describe"}'));
  const views = await client.views(); let rejected = 0; const items: ViewContribution[] = [], duplicate = new Set<string>(), seen = new Set<string>();
  for (const reply of replies) {
    try {
      const data: unknown = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(reply.payload));
      if (!object(data) || Object.keys(data).sort().join(',') !== ['id', labelKey, 'order'].sort().join(',') || typeof data.id !== 'string' || !/^[\x21-\x7e]{1,64}$/.test(data.id) || typeof data[labelKey] !== 'string' || !(data[labelKey] as string).trim() || new TextEncoder().encode(data[labelKey] as string).length > 128 || typeof data.order !== 'number' || !Number.isInteger(data.order) || Math.abs(data.order) > 1000) { rejected++; continue; }
      const candidates = views.filter(view => view.restoreKey.instanceId === reply.provider.instanceId && view.restoreKey.componentId === reply.provider.componentId && view.activation === reply.provider.activation);
      if (candidates.length !== 1) { rejected++; continue; }
      const view = candidates[0], id = `${view.id}/${data.id}`;
      if (seen.has(id)) { duplicate.add(id); rejected++; continue; } seen.add(id);
      items.push({ id, label: data[labelKey] as string, order: data.order, view });
    } catch { rejected++; /* Malformed extension data cannot supply a route. */ }
  }
  return { items: items.filter(item => !duplicate.has(item.id)).sort((a, b) => a.order - b.order || a.label.localeCompare(b.label) || a.id.localeCompare(b.id)), rejected };
}
