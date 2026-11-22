import type { SerializedDockview } from 'dockview-react';
import type { RestoreKey } from '@rintawa/bridge';
export type SavedPanel = { panelId: string; restoreKey: RestoreKey };
export type SavedLayout = { version: 1; panels: SavedPanel[]; dockview: SerializedDockview };
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
const id = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && value.length <= 64 && /^[\x00-\x7f]+$/.test(value);
const keys = (value: Record<string, unknown>, allowed: string[]) => Object.keys(value).every(key => allowed.includes(key));
const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value) && value >= 0;
function depth(value: unknown, n = 0): boolean { return n <= 32 && (!value || typeof value !== 'object' || Object.values(value).every(child => depth(child, n + 1))); }
function restoreKey(value: unknown): value is RestoreKey {
  return record(value) && keys(value, ['worldId', 'instanceId', 'componentId']) &&
    (value.worldId === null || (typeof value.worldId === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value.worldId))) &&
    typeof value.instanceId === 'string' && !!value.instanceId && typeof value.componentId === 'string' && !!value.componentId;
}
/** Validate the pinned DockView profile before handing inert data to fromJSON. */
export function readLayout(value: unknown): SavedLayout | null {
  try {
    if (!depth(value)) return null;
    const encoded = JSON.stringify(value);
    if (new TextEncoder().encode(encoded).length > 65536) return null;
    // Dockview's live toJSON object includes undefined optional properties;
    // validate the same inert JSON representation that preferences persist.
    value = JSON.parse(encoded);
    if (!record(value) || !keys(value, ['version', 'panels', 'dockview']) || value.version !== 1 || !Array.isArray(value.panels) || value.panels.length > 32 || !depth(value) || new TextEncoder().encode(JSON.stringify(value)).length > 65536) return null;
    const panelIds = new Set<string>();
    for (const panel of value.panels) {
      if (!record(panel) || !keys(panel, ['panelId', 'restoreKey']) || !id(panel.panelId) || panelIds.has(panel.panelId) || !restoreKey(panel.restoreKey)) return null;
      panelIds.add(panel.panelId);
    }
    const dock = value.dockview;
    if (!record(dock) || !keys(dock, ['grid', 'panels', 'activeGroup', 'floatingGroups', 'popoutGroups', 'edgeGroups']) || !record(dock.grid) || !record(dock.panels)) return null;
    if ((dock.floatingGroups !== undefined && (!Array.isArray(dock.floatingGroups) || dock.floatingGroups.length)) || (dock.popoutGroups !== undefined && (!Array.isArray(dock.popoutGroups) || dock.popoutGroups.length)) || (dock.edgeGroups !== undefined && (!record(dock.edgeGroups) || Object.keys(dock.edgeGroups).length))) return null;
    if (Object.keys(dock.panels).length !== panelIds.size) return null;
    for (const [key, panel] of Object.entries(dock.panels)) {
      if (!panelIds.has(key) || !record(panel) || !keys(panel, ['id', 'contentComponent', 'title', 'renderer', 'params', 'minimumWidth', 'minimumHeight', 'maximumWidth', 'maximumHeight', 'pinned']) || panel.id !== key || panel.contentComponent !== 'extension') return null;
      if (panel.params !== undefined && (!record(panel.params) || Object.keys(panel.params).length)) return null;
      if (panel.title !== undefined && (typeof panel.title !== 'string' || panel.title.length > 1024)) return null;
      if (panel.renderer !== undefined && !['always', 'onlyWhenVisible'].includes(String(panel.renderer))) return null;
      if (panel.pinned !== undefined && typeof panel.pinned !== 'boolean') return null;
      for (const name of ['minimumWidth', 'minimumHeight', 'maximumWidth', 'maximumHeight']) if (panel[name] !== undefined && !finite(panel[name])) return null;
    }
    const seen = new Set<string>(); const groups = new Set<string>();
    const visit = (node: unknown): boolean => {
      if (!record(node) || !keys(node, ['type', 'data', 'size', 'visible']) || (node.size !== undefined && !finite(node.size)) || (node.visible !== undefined && typeof node.visible !== 'boolean')) return false;
      if (node.type === 'branch') return Array.isArray(node.data) && node.data.length <= 32 && node.data.every(visit);
      if (node.type !== 'leaf' || !record(node.data)) return false;
      const group = node.data;
      if (!keys(group, ['id', 'views', 'activeView', 'locked', 'hideHeader', 'headerPosition', 'skipSetActive', 'constraints', 'initialWidth', 'initialHeight', 'tabGroups']) || !id(group.id) || groups.has(group.id) || !Array.isArray(group.views)) return false;
      groups.add(group.id);
      for (const panel of group.views) { if (!id(panel) || !panelIds.has(panel) || seen.has(panel)) return false; seen.add(panel); }
      if (group.activeView !== undefined && !group.views.includes(group.activeView)) return false;
      if (group.locked !== undefined && typeof group.locked !== 'boolean' && group.locked !== 'no-drop-target') return false;
      for (const field of ['hideHeader', 'skipSetActive']) if (group[field] !== undefined && typeof group[field] !== 'boolean') return false;
      if (group.headerPosition !== undefined && !['top', 'bottom', 'left', 'right'].includes(String(group.headerPosition))) return false;
      for (const field of ['initialWidth', 'initialHeight']) if (group[field] !== undefined && !finite(group[field])) return false;
      if (group.constraints !== undefined && (!record(group.constraints) || !keys(group.constraints, ['minimumWidth', 'minimumHeight', 'maximumWidth', 'maximumHeight']) || !Object.values(group.constraints).every(finite))) return false;
      if (group.tabGroups !== undefined && (!Array.isArray(group.tabGroups) || group.tabGroups.length)) return false;
      return true;
    };
    if (!keys(dock.grid, ['root', 'height', 'width', 'orientation']) || !finite(dock.grid.height) || !finite(dock.grid.width) || !['HORIZONTAL', 'VERTICAL'].includes(String(dock.grid.orientation)) || !visit(dock.grid.root) || seen.size !== panelIds.size) return null;
    if (dock.activeGroup !== undefined && (typeof dock.activeGroup !== 'string' || !groups.has(dock.activeGroup))) return null;
    return value as SavedLayout;
  } catch { return null; }
}
