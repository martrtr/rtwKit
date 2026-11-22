import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react';
import { DockviewReact, themeDark, themeLight, type DockviewApi, type DockviewReadyEvent, type IDockviewPanelProps } from 'dockview-react';
import { RootBridge, resolveView, type View, type Theme } from '@rintawa/bridge';
import { Button, Status } from '@rintawa/ui';
import { readLayout, type SavedPanel, type SavedLayout } from './layout';

type World = { worldId: string; title: string; active: boolean; pendingActive: boolean | null; lastError: string | null };
type Workspace = { bridge: RootBridge; entries: Map<string, SavedPanel>; views: View[]; selectedWorld: string | null };
const Context = createContext<Workspace | null>(null);
const errorText = (error: unknown) => error instanceof Error ? error.message : 'Operation unavailable';
function ExtensionPanel({ api }: IDockviewPanelProps) {
  const workspace = useContext(Context)!; const container = useRef<HTMLDivElement>(null); const [failure, setFailure] = useState(''); const [attempt, setAttempt] = useState(0);
  const entry = workspace.entries.get(api.id); const view = entry ? resolveView(entry.restoreKey, workspace.views) : null;
  const matches = !!entry && entry.restoreKey.worldId === workspace.selectedWorld;
  useEffect(() => {
    if (!view || !matches || !container.current) return;
    let disposed = false; let mounted: { close: () => Promise<void> } | undefined; setFailure('');
    workspace.bridge.mount(view.id, container.current, () => { if (!disposed) setFailure('This view closed its channel.'); }).then(binding => {
      if (disposed) void binding.close().catch(() => {}); else mounted = binding;
    }).catch(error => { if (!disposed) setFailure(errorText(error)); });
    return () => { disposed = true; void mounted?.close().catch(() => {}); };
  }, [workspace.bridge, view?.id, matches, attempt]);
  return <div className="extension-panel"><div ref={container} className="frame-container" />{(!view || !matches || failure) && <div className="panel-placeholder">
    <h2>{!matches ? 'Another World' : !view ? 'View unavailable' : 'View disconnected'}</h2>
    <p>{!matches ? 'Select the saved World to open this view.' : !view ? 'The saved component is missing, inactive or ambiguous.' : failure}</p>
    {!!view && matches && <Button onClick={() => setAttempt(value => value + 1)}>Reopen view</Button>}
  </div>}</div>;
}
const components = { extension: ExtensionPanel };
export function App({ bridge }: { bridge: RootBridge }) {
  const [worlds, setWorlds] = useState<World[]>([]); const [views, setViews] = useState<View[]>([]); const [selectedWorld, setSelectedWorld] = useState<string | null>(null);
  const [entries, setEntries] = useState(new Map<string, SavedPanel>()); const entriesRef = useRef(entries); const [theme, setTheme] = useState<Theme>({ mode: 'dark' });
  const [notice, setNotice] = useState(''); const [error, setError] = useState(''); const [busy, setBusy] = useState(false); const dock = useRef<DockviewApi | null>(null);
  const ready = useRef(false); const canSave = useRef(false); const timer = useRef<ReturnType<typeof setTimeout> | null>(null); const [loaded, setLoaded] = useState(false); const saved = useRef<SavedLayout | null>(null);
  const updateEntries = useCallback((value: Map<string, SavedPanel>) => { entriesRef.current = value; setEntries(value); }, []);
  const refresh = useCallback(async () => {
    const catalog = await bridge.list(); const worlds = await bridge.client.request('world-sessions.list-worlds') as World[];
    setViews(catalog); setWorlds(worlds); return { catalog, worlds };
  }, [bridge]);
  useEffect(() => {
    let live = true;
    (async () => {
      const { worlds } = await refresh(); const value = await bridge.getLayout(); const layout = value === null ? null : readLayout(value);
      if (!live) return;
      saved.current = layout; updateEntries(new Map(layout?.panels.map(panel => [panel.panelId, panel])));
      setSelectedWorld(layout?.panels[0]?.restoreKey.worldId ?? worlds[0]?.worldId ?? null);
      if (value !== null && !layout) setError('Saved layout is unsupported. Reset it to start fresh.');
      setLoaded(true);
    })().catch(error => { if (live) setError(errorText(error)); });
    return () => { live = false; };
  }, [bridge, refresh, updateEntries]);
  useEffect(() => { bridge.theme(theme); }, [bridge, theme]);
  useEffect(() => {
    if (!worlds.some(world => world.pendingActive !== null)) return;
    const timer = setTimeout(() => { void refresh().catch(error => setError(errorText(error))); }, 500); return () => clearTimeout(timer);
  }, [worlds, refresh]);
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, []);
  const scheduleSave = useCallback(() => {
    if (!ready.current || !canSave.current || !dock.current) return;
    if (timer.current) clearTimeout(timer.current); setNotice('Saving layout…');
    timer.current = setTimeout(() => {
      const api = dock.current; if (!api) return;
      const panels = api.panels.map(panel => entriesRef.current.get(panel.id)).filter((p): p is SavedPanel => !!p);
      const value: SavedLayout = { version: 1, panels, dockview: api.toJSON() };
      if (!readLayout(value)) { setError('Layout could not be validated.'); return; }
      bridge.setLayout(value).then(() => setNotice('Layout saved')).catch(error => { setError(`Layout save failed: ${errorText(error)}`); setNotice(''); });
    }, 350);
  }, [bridge]);
  const onReady = useCallback(({ api }: DockviewReadyEvent) => {
    dock.current = api;
    if (saved.current) { try { api.fromJSON(saved.current.dockview); } catch { api.clear(); updateEntries(new Map()); setError('Saved layout could not be restored. Reset it to start fresh.'); } }
    ready.current = true;
    api.onDidRemovePanel(panel => { const entries = new Map(entriesRef.current); entries.delete(panel.id); updateEntries(entries); scheduleSave(); });
    api.onDidLayoutChange(scheduleSave);
  }, [scheduleSave, updateEntries]);
  const open = (view: View, split: boolean) => {
    const api = dock.current; if (!api || api.panels.length >= 32) { setError('The workspace supports up to 32 panels.'); return; }
    const panelId = crypto.randomUUID(); const entries = new Map(entriesRef.current); entries.set(panelId, { panelId, restoreKey: view.restoreKey }); updateEntries(entries); canSave.current = true;
    api.addPanel({ id: panelId, title: view.title, component: 'extension', renderer: 'always', ...(split && api.activePanel ? { position: { referencePanel: api.activePanel.id, direction: 'right' as const } } : {}) }); scheduleSave();
  };
  const action = async (work: () => Promise<unknown>) => { setBusy(true); setError(''); try { await work(); await refresh(); } catch (error) { setError(errorText(error)); } finally { setBusy(false); } };
  const createWorld = () => action(async () => { const world = await bridge.client.request('world-sessions.create') as World; setSelectedWorld(world.worldId); });
  const selected = worlds.find(world => world.worldId === selectedWorld); const available = views.filter(view => view.worldId === selectedWorld && selectedWorld !== null);
  return <Context.Provider value={{ bridge, entries, views, selectedWorld }}><main className="shell">
    <header className="shell-header"><div className="wordmark"><b>R</b><strong>Rintawa</strong><span>World workspace</span></div><nav><Button onClick={() => setTheme({ mode: theme.mode === 'dark' ? 'light' : 'dark' })}>{theme.mode === 'dark' ? 'Light theme' : 'Dark theme'}</Button><Button onClick={() => void bridge.logout().then(() => location.reload())}>Log out</Button></nav></header>
    <aside className="sidebar"><div className="section-title"><h2>Worlds</h2><Button disabled={busy || !loaded} onClick={() => void createWorld()}>New World</Button></div>
      <div className="world-list">{worlds.map(world => <button className={`world-row ${selectedWorld === world.worldId ? 'selected' : ''}`} key={world.worldId} disabled={!loaded || busy} onClick={() => setSelectedWorld(world.worldId)}><span className={`world-dot ${world.active ? 'active' : ''}`} /><span>{world.title}</span><small>{world.pendingActive !== null ? 'pending' : world.active ? 'active' : 'inactive'}</small></button>)}</div>
      {selected && <section className="world-controls"><strong>{selected.title}</strong><Button disabled={busy || selected.pendingActive !== null} onClick={() => void action(() => bridge.client.request('world-sessions.set-active', { worldId: selected.worldId, active: !selected.active }))}>{selected.active ? 'Stop World' : 'Start World'}</Button>{selected.lastError && <Status error>{selected.lastError}</Status>}</section>}
      <div className="section-title"><h2>Views</h2><Button disabled={busy || !loaded} onClick={() => void action(refresh)}>Refresh</Button></div>
      {available.map(view => <div className="view-row" key={view.id}><span>{view.title}</span><Button title={`Open ${view.title}`} onClick={() => open(view, false)}>Open</Button><Button title={`Split ${view.title}`} onClick={() => open(view, true)}>Split</Button></div>)}
      {!available.length && <Status>{selected?.active ? 'No active views in this World.' : 'Start a World to open its views.'}</Status>}
      <div className="sidebar-footer"><Button disabled={busy || !loaded} onClick={() => void action(async () => { await bridge.resetLayout(); saved.current = null; canSave.current = false; dock.current?.clear(); updateEntries(new Map()); setNotice('Layout reset'); })}>Reset layout</Button><Status>{notice}</Status></div>
    </aside>
    <section className="workspace" onPointerDown={() => { canSave.current = true; }} onKeyDown={() => { canSave.current = true; }}>
      <div className="workspace-heading"><span>{selected?.title ?? 'Choose a World'}</span><small>Drag tabs to arrange your workspace</small></div>
      {error && <div className="error-banner" role="alert">{error}<Button onClick={() => location.reload()}>Reconnect</Button></div>}
      <div className="dock-area">{loaded && <DockviewReact components={components} onReady={onReady} theme={theme.mode === 'dark' ? themeDark : themeLight} defaultRenderer="always" disableFloatingGroups getTabContextMenuItems={() => ['close', 'closeOthers']} getTabGroupChipContextMenuItems={() => ['close']} />}</div>
    </section>
  </main></Context.Provider>;
}
