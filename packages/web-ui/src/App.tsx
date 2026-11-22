import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react';
import { DockviewReact, themeDark, themeLight, type DockviewApi, type DockviewReadyEvent, type IDockviewPanelProps } from 'dockview-react';
import { BridgeError, RootBridge, resolveView, type View, type Theme, type ViewState } from '@rintawa/bridge';
import { Button, Dialog, Icon, Input, Status } from '@rintawa/ui';
import logo from './assets/logo.png';
import { readLayout, type SavedPanel, type SavedLayout } from './layout';

type Template = { digest: string; name: string; description: string; invalidReason: string | null };
type TemplateInfo = { components: { slot: string; packageId: string | null; version: string | null }[]; composition: { requestedGrants: { slot: string; componentId: string; permission: string }[] }; hostRequirements: { artifact: string; componentId: string; target: string }[] };
type World = { worldId: string; title: string; description?: string | null; cover?: unknown; active: boolean; pendingActive: boolean | null; lastError: string | null };
type Workspace = { bridge: RootBridge; entries: Map<string, SavedPanel>; views: View[]; selectedWorld: string | null; initialStates: Map<string, ViewState> };
const Context = createContext<Workspace | null>(null);
const errorText = (error: unknown) => error instanceof Error ? error.message : 'Operation unavailable';
async function readHost(bridge: RootBridge, method: string, args: Record<string, unknown> = {}): Promise<unknown> {
  for (let attempt = 0; ; attempt++) {
    try { return await bridge.client.request(method, args); }
    catch (error) {
      if (attempt >= 5 || !(error instanceof BridgeError) || !['unavailable', 'limit-exceeded'].includes(error.code)) throw error;
      await new Promise(resolve => setTimeout(resolve, 80));
    }
  }
}
function ExtensionPanel({ api }: IDockviewPanelProps) {
  const workspace = useContext(Context)!; const container = useRef<HTMLDivElement>(null); const [failure, setFailure] = useState(''); const [attempt, setAttempt] = useState(0);
  const entry = workspace.entries.get(api.id); const view = entry ? resolveView(entry.restoreKey, workspace.views) : null;
  const matches = !!entry && entry.restoreKey.worldId === workspace.selectedWorld;
  useEffect(() => {
    if (!view || !matches || !container.current) return;
    let disposed = false; let mounted: { close: () => Promise<void> } | undefined; setFailure('');
    workspace.bridge.mount(view.id, container.current, () => { if (!disposed) setFailure('This view closed its channel.'); }, workspace.initialStates.get(api.id)).then(binding => {
      if (disposed) void binding.close().catch(() => {}); else mounted = binding;
    }).catch(error => { if (!disposed) setFailure(errorText(error)); });
    return () => { disposed = true; void mounted?.close().catch(() => {}); };
  }, [workspace.bridge, view?.id, matches, attempt, workspace.initialStates.get(api.id)]);
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
  const [templates, setTemplates] = useState<Template[]>([]); const [template, setTemplate] = useState(''); const [templateInfo, setTemplateInfo] = useState<TemplateInfo | null>(null); const [approved, setApproved] = useState(false);
  const [entryByWorld, setEntryByWorld] = useState(new Map<string, { instanceId: string; componentId: string } | null>());
  const [initialStates, setInitialStates] = useState(new Map<string, ViewState>());
  const openRef = useRef<(view: View, split: boolean, state?: ViewState) => void>(() => {});
  const selectedRef = useRef(selectedWorld); selectedRef.current = selectedWorld;
  const [dockReady, setDockReady] = useState(false);
  const [createOpen, setCreateOpen] = useState(false); const [worldName, setWorldName] = useState(''); const [search, setSearch] = useState(''); const [toolsOpen, setToolsOpen] = useState(false);
  const [readError, setReadError] = useState('');
  const [notice, setNotice] = useState(''); const [error, setError] = useState(''); const [busy, setBusy] = useState(false); const dock = useRef<DockviewApi | null>(null);
  const activeByWorld = useRef(new Map<string, string>());
  const ready = useRef(false); const canSave = useRef(false); const timer = useRef<ReturnType<typeof setTimeout> | null>(null); const [loaded, setLoaded] = useState(false); const saved = useRef<SavedLayout | null>(null);
  const updateEntries = useCallback((value: Map<string, SavedPanel>) => { entriesRef.current = value; setEntries(value); }, []);
  const refresh = useCallback(async () => {
    const catalog = await readHost(bridge, 'ui.list') as View[]; const worlds = await readHost(bridge, 'world-sessions.list-worlds') as World[];
    setViews(catalog); setWorlds(worlds); setReadError(''); return { catalog, worlds };
  }, [bridge]);
  useEffect(() => {
    let live = true;
    (async () => {
      const { worlds } = await refresh(); const value = await bridge.getLayout(); const layout = value === null ? null : readLayout(value);
      if (!live) return;
      saved.current = layout; updateEntries(new Map(layout?.panels.map(panel => [panel.panelId, panel])));
      setSelectedWorld(layout?.panels[0]?.restoreKey.worldId ?? null);
      if (value !== null && !layout) setError('Saved layout is unsupported. Reset it to start fresh.');
      setLoaded(true);
    })().catch(error => { if (live) setError(errorText(error)); });
    return () => { live = false; };
  }, [bridge, refresh, updateEntries]);
  useEffect(() => { bridge.theme(theme); }, [bridge, theme]);
  useEffect(() => bridge.onDisconnect(() => setError('Connection closed. Reconnect to continue.')), [bridge]);
  useEffect(() => {
    if (!worlds.some(world => world.pendingActive !== null)) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try { await refresh(); }
      catch (failure) {
        // Lifecycle work can delay the pump. Retry transient reads, never set-active.
        if (!(failure instanceof BridgeError) || !['unavailable', 'limit-exceeded'].includes(failure.code)) {
          if (!cancelled) setError(errorText(failure));
          cancelled = true;
        }
      }
      finally { if (!cancelled) timer = setTimeout(() => void poll(), 500); }
    };
    timer = setTimeout(() => void poll(), 500);
    return () => { cancelled = true; clearTimeout(timer); };
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
    ready.current = true; setDockReady(true);
    api.onDidActivePanelChange(({ panel }) => { if (panel) { const key = entriesRef.current.get(panel.id)?.restoreKey; if (key?.worldId) activeByWorld.current.set(key.worldId, panel.id); } });
    api.onDidRemovePanel(panel => { for (const [world, id] of activeByWorld.current) if (id === panel.id) activeByWorld.current.delete(world); const entries = new Map(entriesRef.current); entries.delete(panel.id); updateEntries(entries); scheduleSave(); });
    api.onDidLayoutChange(scheduleSave);
  }, [scheduleSave, updateEntries]);
  const open = (view: View, split: boolean, state?: ViewState) => {
    const api = dock.current; if (!api) return;
    if (!split) {
      const existing = api.panels.find(panel => { const key = entriesRef.current.get(panel.id)?.restoreKey; return key?.worldId === view.restoreKey.worldId && key.instanceId === view.restoreKey.instanceId && key.componentId === view.restoreKey.componentId; });
      if (existing) { if (state) setInitialStates(previous => new Map(previous).set(existing.id, state)); existing.api.setActive(); return; }
    }
    if (api.panels.length >= 32) { setError('The workspace supports up to 32 panels.'); return; }
    const panelId = crypto.randomUUID(); const entries = new Map(entriesRef.current); entries.set(panelId, { panelId, restoreKey: view.restoreKey }); updateEntries(entries); canSave.current = true;
    if (state) setInitialStates(previous => new Map(previous).set(panelId, state));
    api.addPanel({ id: panelId, title: view.title, component: 'extension', renderer: 'always', ...(split && api.activePanel ? { position: { referencePanel: api.activePanel.id, direction: 'right' as const } } : {}) }); scheduleSave();
  };
  openRef.current = open;
  useEffect(() => bridge.onNavigate((view, state, source) => {
    if (view.worldId !== selectedRef.current) return;
    const current = dock.current?.activePanel;
    const key = current && entriesRef.current.get(current.id)?.restoreKey;
    openRef.current(view, false, state);
    if (current && key?.worldId === source.worldId && key.instanceId === source.restoreKey.instanceId && key.componentId === source.restoreKey.componentId && source.id !== view.id) dock.current?.removePanel(current);
  }), [bridge]);
  useEffect(() => {
    if (!createOpen) return;
    let cancelled = false; setTemplateInfo(null); setApproved(false);
    readHost(bridge, 'world-sessions.list-templates').then(rows => { if (!cancelled) { const catalog = rows as Template[]; setTemplates(catalog); setTemplate(previous => catalog.some(row => row.digest === previous && !row.invalidReason) ? previous : catalog.find(row => !row.invalidReason)?.digest ?? ''); } }).catch(error => { if (!cancelled) setError(errorText(error)); });
    return () => { cancelled = true; };
  }, [bridge, createOpen]);
  useEffect(() => {
    let cancelled = false; setTemplateInfo(null); setApproved(false);
    if (createOpen && template) readHost(bridge, 'world-sessions.template-info', { digest: template }).then(value => { if (!cancelled) setTemplateInfo(value as TemplateInfo); }).catch(error => { if (!cancelled) setError(errorText(error)); });
    return () => { cancelled = true; };
  }, [bridge, createOpen, template]);
  useEffect(() => {
    if (!selectedWorld) return;
    let cancelled = false; let timer: ReturnType<typeof setTimeout>;
    const read = async () => {
      try { const value = await readHost(bridge, 'world-sessions.entry', { worldId: selectedWorld }); if (!cancelled) setEntryByWorld(previous => new Map(previous).set(selectedWorld, value as { instanceId: string; componentId: string } | null)); }
      catch (error) { if (!cancelled) { setReadError(errorText(error)); if (error instanceof BridgeError && ['unavailable', 'limit-exceeded'].includes(error.code)) timer = setTimeout(() => void read(), 500); } }
    };
    void read(); return () => { cancelled = true; clearTimeout(timer); };
  }, [bridge, selectedWorld]);
  useEffect(() => {
    if (!dockReady || !selectedWorld || !worlds.some(world => world.worldId === selectedWorld && world.active) || [...entriesRef.current.values()].some(panel => panel.restoreKey.worldId === selectedWorld)) return;
    const entry = entryByWorld.get(selectedWorld); if (!entry) return;
    const view = resolveView({ worldId: selectedWorld, ...entry }, views);
    if (view) { openRef.current(view, false); return; }
    const timer = setInterval(() => void refresh().catch(error => setReadError(errorText(error))), 500);
    return () => clearInterval(timer);
  }, [dockReady, selectedWorld, worlds, views, entryByWorld, refresh]);
  const action = async (work: () => Promise<unknown>) => { setBusy(true); setError(''); try { await work(); await refresh().catch(error => setReadError(errorText(error))); } catch (error) { setError(errorText(error)); } finally { setBusy(false); } };
  const selectWorld = (id: string) => {
    setSelectedWorld(id); setToolsOpen(false);
    const existing = dock.current?.panels.find(panel => panel.id === activeByWorld.current.get(id)) ?? dock.current?.panels.find(panel => entriesRef.current.get(panel.id)?.restoreKey.worldId === id);
    existing?.api.setActive();
  };
  const createWorld = () => action(async () => {
    if (!worldName.trim() || new TextEncoder().encode(worldName.trim()).length > 256) throw new Error('Enter a World name of up to 256 UTF-8 bytes.');
    if (!template || !templateInfo) throw new Error('Choose an installed template.');
    const world = await bridge.client.request('world-sessions.create-from-template', { digest: template, title: worldName.trim(), approveRuntimePermissions: approved }) as World;
    setWorlds(rows => [...rows, world]); selectWorld(world.worldId); setCreateOpen(false);
    await setWorldActive(world);
    setWorldName('');
  });
  const setWorldActive = async (world: World) => {
    setBusy(true); setError('');
    // Retain a reconciliation obligation even if the admission reply is lost.
    // A successful read replaces this local pending hint with authoritative state.
    setWorlds(rows => rows.map(row => row.worldId === world.worldId ? { ...row, pendingActive: !world.active } : row));
    try { await bridge.client.request('world-sessions.set-active', { worldId: world.worldId, active: !world.active }); }
    catch (error) {
      if (error instanceof BridgeError && error.outcome === 'unknown') setNotice('Checking World status…');
      else setError(errorText(error));
    }
    finally { setBusy(false); }
  };
  const selected = worlds.find(world => world.worldId === selectedWorld); const available = views.filter(view => view.worldId === selectedWorld && selectedWorld !== null);
  const home = selectedWorld === null;
  const initials = (name: string) => name.trim().split(/\s+/).slice(0, 2).map(word => Array.from(word)[0]).join('').toUpperCase();
  const worldPanels = [...entries.values()].filter(panel => panel.restoreKey.worldId === selectedWorld);
  // The main bar follows opened views. The template entry is an onboarding route,
  // available through workspace controls rather than a duplicate app shortcut.
  const entry = selectedWorld ? entryByWorld.get(selectedWorld) : null;
  const navigationViews = available.filter(view =>
    !(entry && view.restoreKey.instanceId === entry.instanceId && view.restoreKey.componentId === entry.componentId)
    && worldPanels.some(panel => panel.restoreKey.instanceId === view.restoreKey.instanceId && panel.restoreKey.componentId === view.restoreKey.componentId));
  const filtered = worlds.filter(world => world.title.toLowerCase().includes(search.toLowerCase()));
  return <Context.Provider value={{ bridge, entries, views, selectedWorld, initialStates }}><main className="shell">
    <nav className="world-rail" aria-label="Worlds">
      <button className={`home-button rail-button${home ? ' selected' : ''}`} aria-label="Home" title="Home" onClick={() => { setSelectedWorld(null); setToolsOpen(false); }}><img src={logo} alt="Rintawa" /></button>
      <div className="rail-divider" />
      <div className="rail-worlds">{worlds.map(world => <button key={world.worldId} title={world.title} aria-label={`Open world ${world.title}`} aria-current={selectedWorld === world.worldId ? 'page' : undefined} disabled={!loaded} className={`world-button rail-button${selectedWorld === world.worldId ? ' selected' : ''}`} onClick={() => selectWorld(world.worldId)}><span>{initials(world.title) || <Icon name="world" />}</span>{world.active && <i className="active-dot" aria-label="Active" />}</button>)}</div>
      <button className="rail-button add-world" title="New World" aria-label="New World" disabled={!loaded || busy} onClick={() => setCreateOpen(true)}><Icon name="plus" size={24} /></button>
      <div className="rail-bottom"><button className="rail-button utility" aria-label="Log out" title="Log out" onClick={() => void bridge.logout().then(() => location.reload())}><Icon name="logout" /></button></div>
    </nav>
    <section className="shell-body">
      {home && <section className="home-page" aria-label="Home"><header className="home-topbar"><span>Home</span><button className="icon-button" aria-label="Refresh worlds" onClick={() => void action(refresh)}><Icon name="refresh" size={18} /></button></header>
        <div className="home-content"><div className="home-intro"><img src={logo} alt="" /><div><h1>Welcome to Rintawa.</h1></div></div>
          <div className="worlds-heading"><div><h2>Your worlds</h2><p>Characters, conversations, and everything between.</p></div><Button className="primary" disabled={!loaded || busy} onClick={() => setCreateOpen(true)}><Icon name="plus" size={17} /> Create a world</Button></div>
          {!!worlds.length && <label className="world-search"><Icon name="search" size={17} /><input value={search} onChange={event => setSearch(event.target.value)} placeholder="Find a world" aria-label="Find a world" /></label>}
          {!loaded ? <Status>Loading worlds…</Status> : filtered.length ? <div className="world-grid">{filtered.map(world => <button className="world-card" key={world.worldId} onClick={() => selectWorld(world.worldId)}><div className="world-card-icon">{initials(world.title)}</div><div className="world-card-copy"><h3>{world.title}</h3><p>{world.description || 'A space for your characters and conversations.'}</p><span className={world.active ? 'world-state online' : 'world-state'}>{world.active ? 'Active' : 'Not running'}</span></div><Icon name="arrow" size={18} /></button>)}</div> : <div className="home-empty"><div className="empty-symbol"><Icon name="world" size={36} /></div><h3>{search ? 'No worlds found' : 'Your first world starts here'}</h3><p>{search ? 'Try another name.' : 'Create a world, bring in a character, and start a conversation.'}</p>{!search && <Button className="primary" onClick={() => setCreateOpen(true)}><Icon name="plus" size={16} /> Create a world</Button>}</div>}

        </div>
      </section>}
      <section className={`world-workspace${home ? ' hidden-workspace' : ''}`} aria-hidden={home}>
        <header className="world-topbar"><div className="world-title"><Icon name="world" size={18} /><strong>{selected?.title ?? 'World unavailable'}</strong></div><nav className="world-apps" aria-label="World apps">{navigationViews.map(view => <button key={view.id} title={`Open ${view.title}`} onClick={() => open(view, false)}>{view.title}</button>)}</nav><div className="world-actions"><button className="icon-button" title="Refresh" aria-label="Refresh" disabled={busy || !loaded} onClick={() => void action(refresh)}><Icon name="refresh" size={17} /></button>{selected && <button className={`world-power${selected.active ? ' running' : ''}`} disabled={busy || selected.pendingActive !== null} onClick={() => void setWorldActive(selected)}><Icon name="power" size={15} />{selected.pendingActive !== null ? 'Working…' : selected.active ? 'Stop World' : 'Start World'}</button>}<button className="icon-button" title="Workspace controls" aria-label="Workspace controls" aria-expanded={toolsOpen} onClick={() => setToolsOpen(value => !value)}><Icon name="more" /></button></div></header>
        {selected?.lastError && <Status error>{selected.lastError}</Status>}
        {toolsOpen && <div className="workspace-tools"><span>Workspace</span>{available.map(view => <Button key={view.id} title={`Split ${view.title}`} onClick={() => open(view, true)}><Icon name="split" size={15} /> Split {view.title}</Button>)}<Button onClick={() => setTheme({ mode: theme.mode === 'dark' ? 'light' : 'dark' })}>{theme.mode === 'dark' ? 'Light theme' : 'Dark theme'}</Button><Button onClick={() => void action(async () => { await bridge.resetLayout(); saved.current = null; canSave.current = false; dock.current?.clear(); updateEntries(new Map()); setNotice('Layout reset'); })}>Reset layout</Button>{notice && <Status>{notice}</Status>}</div>}
        <div className={`dock-area${worldPanels.length <= 1 && !toolsOpen ? ' single-panel' : ''}`}>{loaded && <DockviewReact components={components} onReady={onReady} theme={theme.mode === 'dark' ? themeDark : themeLight} defaultRenderer="always" disableFloatingGroups getTabContextMenuItems={() => ['close', 'closeOthers']} getTabGroupChipContextMenuItems={() => ['close']} />}</div>
        {!worldPanels.length && !home && <div className="world-welcome"><div className="empty-symbol"><Icon name="world" size={34} /></div><h1>{selected?.title ?? 'World unavailable'}</h1><p>{selected?.active ? 'Choose an app to step into this world.' : 'Start this world to open its chats and characters.'}</p>{selected?.active ? <div className="world-launchers">{available.map(view => <Button key={view.id} title={`Open ${view.title}`} onClick={() => open(view, false)}><Icon name="grid" size={18} /> {view.title}<Icon name="arrow" size={16} /></Button>)}{!available.length && <Status>Waiting for World apps…</Status>}</div> : selected && <Button className="primary" disabled={busy || selected.pendingActive !== null} onClick={() => void setWorldActive(selected)}><Icon name="power" size={16} />{selected.pendingActive !== null ? 'Working…' : 'Start World'}</Button>}</div>}
      </section>
      {(error || readError) && <div className="shell-error" role="alert"><span>{error || readError}</span><button onClick={() => { setError(''); setReadError(''); }} aria-label="Dismiss error"><Icon name="close" size={16} /></button><button onClick={() => location.reload()}>Reconnect</button></div>}
    </section>
    <Dialog open={createOpen} title="Create a world" onClose={() => { if (!busy) setCreateOpen(false); }}><form onSubmit={event => { event.preventDefault(); void createWorld(); }}><label htmlFor="world-template">TEMPLATE</label><select id="world-template" value={template} disabled={busy} onChange={event => setTemplate(event.target.value)}>{!templates.length && <option value="">No installed templates</option>}{templates.map(row => <option key={row.digest} value={row.digest} disabled={!!row.invalidReason}>{row.name}{row.invalidReason ? ' · unavailable' : ''}</option>)}</select>
      {templateInfo && <details className="template-requirements"><summary>Included extensions and permissions</summary>{templateInfo.components.map(slot => <p key={slot.slot}>{slot.packageId ?? slot.slot} {slot.version}</p>)}<ul>{templateInfo.composition.requestedGrants.map(grant => <li key={`${grant.slot}/${grant.componentId}/${grant.permission}`}>{grant.slot}/{grant.componentId}: {grant.permission}</li>)}</ul></details>}
      {!!templateInfo?.composition.requestedGrants.length && <label className="template-approval"><input type="checkbox" checked={approved} disabled={busy} onChange={event => setApproved(event.target.checked)} /> Allow the listed permissions in this World</label>}
      <label htmlFor="world-name">WORLD NAME</label><Input id="world-name" value={worldName} onChange={event => setWorldName(event.target.value)} placeholder="Give your world a name" autoFocus disabled={busy} />{error && <Status error>{error}</Status>}<footer><Button disabled={busy} onClick={() => setCreateOpen(false)}>Cancel</Button><Button type="submit" className="primary" disabled={busy || !worldName.trim() || !templateInfo || (!!templateInfo.composition.requestedGrants.length && !approved)}>Create world</Button></footer></form></Dialog>
  </main></Context.Provider>;
}
