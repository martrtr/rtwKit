import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react';
import { DockviewReact, themeDark, themeLight, type DockviewApi, type DockviewReadyEvent, type IDockviewPanelProps } from 'dockview-react';
import { BridgeError, RootBridge, resolveView, type View, type Theme, type ViewState, type PermissionRequest } from '@rintawa/bridge';
import { observeResource, type Observation } from '@rintawa/domain';
import { Button, Dialog, Icon, Input, Status } from '@rintawa/ui';
import logo from './assets/logo.png';
import { WorldActions } from './WorldActions';
import { Surface, selectedView } from './Surface';
import { creationAccess, type CreationPermission, type TemplateComponent } from './creation-access';
import { Permissions } from './Permissions';
import { uploadPackage } from './packages';
import { readLayout, type SavedPanel, type SavedLayout } from './layout';

type Template = { digest: string; name: string; description: string; invalidReason: string | null };
type TemplateInfo = { components: TemplateComponent[]; composition: { requestedGrants: { slot: string; componentId: string; permission: string }[] }; hostRequirements: { artifact: string; componentId: string; target: string }[] };
type World = { worldId: string; title: string; description?: string | null; cover?: unknown; active: boolean; pendingActive: boolean | null; lastError: string | null };
type Workspace = { bridge: RootBridge; entries: Map<string, SavedPanel>; views: View[]; selectedWorld: string | null; initialStates: Map<string, ViewState>; liveWorlds: Set<string> };
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
  const live = !!entry?.restoreKey.worldId && workspace.liveWorlds.has(entry.restoreKey.worldId);
  useEffect(() => {
    if (!view || !matches || !live || !container.current) return;
    const abort = new AbortController(); let disposed = false; let mounted: { close: () => Promise<void> } | undefined; setFailure('');
    workspace.bridge.mount(view.id, container.current, () => { if (!disposed) setFailure('This view closed its channel.'); }, workspace.initialStates.get(api.id), abort.signal).then(binding => {
      if (disposed) void binding.close().catch(() => {}); else mounted = binding;
    }).catch(error => { if (!disposed) setFailure(errorText(error)); });
    return () => { disposed = true; abort.abort(); void mounted?.close().catch(() => {}); };
  }, [workspace.bridge, view?.id, matches, live, attempt, workspace.initialStates.get(api.id)]);
  return <div className="extension-panel"><div ref={container} className="frame-container" />{(!view || !matches || !live || failure) && <div className="panel-placeholder">
    <h2>{!matches ? 'Another World' : !live ? 'World stopped' : !view ? 'View unavailable' : 'View disconnected'}</h2>
    <p>{!matches ? 'Select the saved World to open this view.' : !live ? 'Start this World to reopen its views.' : !view ? 'The saved component is missing, inactive or ambiguous.' : failure}</p>
    {!!view && matches && live && <Button onClick={() => setAttempt(value => value + 1)}>Reopen view</Button>}
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
  const fileInput = useRef<HTMLInputElement>(null); const uploadAbort = useRef<AbortController | null>(null);
  const [uploadBusy, setUploadBusy] = useState(false); const [uploadProgress, setUploadProgress] = useState('');
  useEffect(() => () => uploadAbort.current?.abort(), []);
  const [createOpen, setCreateOpen] = useState(false); const [worldName, setWorldName] = useState('');
  const [creationPermissions,setCreationPermissions]=useState<CreationPermission[]>([]);const [accessReady,setAccessReady]=useState(false);
  const [readError, setReadError] = useState('');
  const [permissionRequest, setPermissionRequest] = useState<{ view: View; request: PermissionRequest } | null>(null);
  const consentPending = useRef(false);
  const closePermissions = useCallback(() => { setPermissionRequest(null); consentPending.current = false; }, []);
  useEffect(() => bridge.onPermissionRequest((view, request) => {
    if (consentPending.current) return false;
    consentPending.current = true; setPermissionRequest({ view, request }); return true;
  }), [bridge]);
  const [settings, setSettings] = useState<{ scope: string | null } | null>(null);
  const [notice, setNotice] = useState(''); const [error, setError] = useState(''); const [busy, setBusy] = useState(false); const dock = useRef<DockviewApi | null>(null);
  const activeByWorld = useRef(new Map<string, string>());
  const ready = useRef(false); const canSave = useRef(false); const timer = useRef<ReturnType<typeof setTimeout> | null>(null); const [loaded, setLoaded] = useState(false); const saved = useRef<SavedLayout | null>(null); const initialized=useRef(false);
  const autoOpenedEntries=useRef(new Set<string>());
  const lastActiveWorlds = useRef(new Set<string>()); const catalogRefreshUntil = useRef(0);
  const updateEntries = useCallback((value: Map<string, SavedPanel>) => { entriesRef.current = value; setEntries(value); }, []);
  const catalogWatch=useRef<Observation|null>(null),catalogValue=useRef<{catalog:View[];worlds:World[]}|null>(null);
  useEffect(()=>{
    const watch=observeResource({client:bridge.client,
      read:async()=>({catalog:await bridge.list(),worlds:await readHost(bridge,'world-sessions.list-worlds') as World[],initialLayout:initialized.current?undefined:await bridge.getLayout()}),
      apply:value=>{
        if(!initialized.current){
          const layout=value.initialLayout===null?null:readLayout(value.initialLayout as string);
          saved.current=layout;updateEntries(new Map(layout?.panels.map(panel=>[panel.panelId,panel])));
          setSelectedWorld(layout?.panels[0]?.restoreKey.worldId??null);
          if(value.initialLayout!==null&&!layout)setError('Saved layout is unsupported. Reset it to start fresh.');
          initialized.current=true;setLoaded(true);
        }
        if(value.worlds.some(world=>world.active&&!lastActiveWorlds.current.has(world.worldId)))catalogRefreshUntil.current=Date.now()+3000;
        for(const world of value.worlds)if(world.active&&!lastActiveWorlds.current.has(world.worldId))autoOpenedEntries.current.delete(world.worldId);
        lastActiveWorlds.current=new Set(value.worlds.filter(world=>world.active).map(world=>world.worldId));
        catalogValue.current=value;setViews(value.catalog);setWorlds(value.worlds);setReadError('');
      },failed:error=>setReadError(errorText(error))});
    catalogWatch.current=watch;return()=>{watch.close();catalogWatch.current=null;};
  },[bridge]);
  const refresh=useCallback(async()=>{await catalogWatch.current!.refresh();return catalogValue.current!;},[]);
  useEffect(() => { bridge.theme(theme); }, [bridge, theme]);
  useEffect(() => bridge.onDisconnect(() => setError('Connection closed. Reconnect to continue.')), [bridge]);
  useEffect(() => {
    if (!worlds.some(world => world.pendingActive !== null) && Date.now() >= catalogRefreshUntil.current) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try { await refresh(); }
      catch (failure) {
        // Lifecycle work can delay the pump; view publication can follow activation.
        // Reconcile status/catalog through reads, never repeat set-active.
        if (!(failure instanceof BridgeError) || !['unavailable', 'limit-exceeded'].includes(failure.code)) {
          if (!cancelled) setError(errorText(failure));
          cancelled = true;
        }
      }
      finally { if (!cancelled && (worlds.some(world => world.pendingActive !== null) || Date.now() < catalogRefreshUntil.current)) timer = setTimeout(() => void poll(), 500); }
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
    api.onDidRemovePanel(panel => { const scope=entriesRef.current.get(panel.id)?.restoreKey.worldId;if(scope)autoOpenedEntries.current.add(scope);for (const [world, id] of activeByWorld.current) if (id === panel.id) activeByWorld.current.delete(world); const entries = new Map(entriesRef.current); entries.delete(panel.id); updateEntries(entries); scheduleSave(); });
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
    const entry=source.worldId?entryByWorld.get(source.worldId):null;
    if (entry?.instanceId===source.restoreKey.instanceId && entry.componentId===source.restoreKey.componentId && current && key?.worldId === source.worldId && key.instanceId === source.restoreKey.instanceId && key.componentId === source.restoreKey.componentId && source.id !== view.id) dock.current?.removePanel(current);
  }), [bridge, entryByWorld]);
  useEffect(() => {
    if (!createOpen) return;
    let cancelled = false; setTemplateInfo(null); setApproved(false);
    readHost(bridge, 'world-sessions.list-templates').then(rows => { if (!cancelled) { const catalog = rows as Template[]; setTemplates(catalog); setTemplate(previous => catalog.some(row => row.digest === previous && !row.invalidReason) ? previous : catalog.find(row => !row.invalidReason)?.digest ?? ''); } }).catch(error => { if (!cancelled) setError(errorText(error)); });
    return () => { cancelled = true; };
  }, [bridge, createOpen]);
  useEffect(() => {
    let cancelled = false; setTemplateInfo(null); setApproved(false);setCreationPermissions([]);setAccessReady(false);
    if (createOpen && template) readHost(bridge, 'world-sessions.template-info', { digest: template }).then(async value => { const info=value as TemplateInfo;const permissions=await creationAccess(bridge,info.components);if(!cancelled){setTemplateInfo(info);setCreationPermissions(permissions);setAccessReady(true);} }).catch(error => { if (!cancelled) setError(errorText(error)); });
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
    if (!dockReady || !selectedWorld || autoOpenedEntries.current.has(selectedWorld) || !worlds.some(world => world.worldId === selectedWorld && world.active) || [...entriesRef.current.values()].some(panel => panel.restoreKey.worldId === selectedWorld)) return;
    const entry = entryByWorld.get(selectedWorld); if (!entry) return;
    const view = resolveView({ worldId: selectedWorld, ...entry }, views);
    if (view) { autoOpenedEntries.current.add(selectedWorld);openRef.current(view, false); return; }
    const timer = setInterval(() => void refresh().catch(error => setReadError(errorText(error))), 500);
    return () => clearInterval(timer);
  }, [dockReady, selectedWorld, worlds, views, entryByWorld, refresh]);
  const action = async (work: () => Promise<unknown>) => { setBusy(true); setError(''); try { await work(); await refresh().catch(error => setReadError(errorText(error))); } catch (error) { setError(errorText(error)); } finally { setBusy(false); } };
  const selectWorld = (id: string) => {
    setSelectedWorld(id);
    const existing = dock.current?.panels.find(panel => panel.id === activeByWorld.current.get(id)) ?? dock.current?.panels.find(panel => entriesRef.current.get(panel.id)?.restoreKey.worldId === id);
    existing?.api.setActive();
  };
  const importTemplate = async (file: File) => {
    const abort = new AbortController(); uploadAbort.current = abort; setUploadBusy(true); setError(''); setUploadProgress('Uploading template…');
    try {
      const digest = await uploadPackage(bridge.client, file, abort.signal, percent => setUploadProgress(percent === 100 ? 'Checking package…' : `Uploading template… ${percent}%`));
      const catalog = await readHost(bridge, 'world-sessions.list-templates') as Template[];
      const selected = catalog.find(row => row.digest === digest);
      if (!selected || selected.invalidReason) throw new Error(selected?.invalidReason ?? 'This RTW is not a World template.');
      if (!abort.signal.aborted) { setTemplates(catalog); setTemplate(digest); setApproved(false); }
    } catch (failure) { if (!abort.signal.aborted) setError(errorText(failure)); }
    finally { if (uploadAbort.current === abort) uploadAbort.current = null; setUploadBusy(false); setUploadProgress(''); }
  };
  const closeCreate = () => { if (busy) return; uploadAbort.current?.abort(); setCreateOpen(false); };
  const removeWorldPanels = async (id: string) => {
    const panels = dock.current?.panels.filter(panel => entriesRef.current.get(panel.id)?.restoreKey.worldId === id) ?? [];
    for (const panel of panels) dock.current?.removePanel(panel);
    const next = new Map(entriesRef.current); for (const [key, panel] of next) if (panel.restoreKey.worldId === id) next.delete(key); updateEntries(next);
    setInitialStates(previous => { const next = new Map(previous); for (const panel of panels) next.delete(panel.id); return next; });
    activeByWorld.current.delete(id); if (selectedRef.current === id) setSelectedWorld(null);
    await refresh();
  };
  const createWorld = () => action(async () => {
    if (!worldName.trim() || new TextEncoder().encode(worldName.trim()).length > 256) throw new Error('Enter a World name of up to 256 UTF-8 bytes.');
    if (!template || !templateInfo) throw new Error('Choose an installed template.');
    const world = await bridge.client.request('world-sessions.create-from-template-with-permissions', {request:{ digest: template, title: worldName.trim(), approveRuntimePermissions: approved, permissions:creationPermissions.map(({label:_,...permission})=>permission) }}) as World;
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
    finally { setBusy(false); void refresh().catch(error=>setReadError(errorText(error))); }
  };
  useEffect(()=>bridge.onIntent(async(method,args,source)=>{
    const provides=(id:string)=>source.provides.some(p=>p.id===id&&p.version===1&&p.selected);
    if(method==='ui.theme.set'){
      if(!provides('rintawa.ui.appearance')||Object.keys(args).join(',')!=='theme'||!args.theme||typeof args.theme!=='object'||Object.keys(args.theme).join(',')!=='mode'||!['dark','light'].some(mode=>(args.theme as Theme).mode===mode))throw new BridgeError('invalid-request','not-started');
      setTheme({mode:(args.theme as Theme).mode});return null;
    }
    if(source.worldId!==null||!provides('rintawa.ui.home.renderer'))throw new BridgeError('invalid-request','not-started');
    if(method==='ui.create-world'&&!Object.keys(args).length){setCreateOpen(true);return null;}
    if(method==='ui.open-world'&&Object.keys(args).join(',')==='worldId'&&typeof args.worldId==='string'){
      const catalog=await readHost(bridge,'world-sessions.list-worlds') as World[];if(!catalog.some(w=>w.worldId===args.worldId))throw new BridgeError('invalid-request','not-started');
      await refresh();selectWorld(args.worldId);return null;
    }
    throw new BridgeError('invalid-request','not-started');
  }),[bridge,refresh]);
  const selected = worlds.find(world => world.worldId === selectedWorld); const available = views.filter(view => view.launchable && view.worldId === selectedWorld && selectedWorld !== null);
  const home = selectedWorld === null;
  const initials = (name: string) => name.trim().split(/\s+/).slice(0, 2).map(word => Array.from(word)[0]).join('').toUpperCase();
  const worldPanels = [...entries.values()].filter(panel => panel.restoreKey.worldId === selectedWorld);
  // The template entry coordinates onboarding and redirects to the actual app.
  // It is not a separate app to reopen or split in the workspace.
  const entry = selectedWorld ? entryByWorld.get(selectedWorld) : null;
  const workspaceViews = available.filter(view =>
    !(entry && view.restoreKey.instanceId === entry.instanceId && view.restoreKey.componentId === entry.componentId));
  const navigationViews = workspaceViews.filter(view =>
    worldPanels.some(panel => panel.restoreKey.instanceId === view.restoreKey.instanceId && panel.restoreKey.componentId === view.restoreKey.componentId));
  const controls = <WorldActions canCloseView={worldPanels.length>0} closeView={()=>{
    const api=dock.current;if(!api)return;
    const active=api.activePanel;
    const panel=active&&entriesRef.current.get(active.id)?.restoreKey.worldId===selectedRef.current?active:api.panels.find(p=>entriesRef.current.get(p.id)?.restoreKey.worldId===selectedRef.current);
    if(panel)api.removePanel(panel);
  }} bridge={bridge} world={home ? null : selected ?? null} views={home ? [] : workspaceViews} settings={() => setSettings({ scope: selectedWorld })} open={view => open(view, true)} refresh={refresh} deleted={removeWorldPanels} stopping={id => setWorlds(rows => rows.map(row => row.worldId === id ? { ...row, pendingActive: false } : row))} fail={failure => setError(errorText(failure))} reset={async () => {
          if (timer.current) { clearTimeout(timer.current); timer.current = null; }
          canSave.current = false; if(selectedRef.current)autoOpenedEntries.current.delete(selectedRef.current);await bridge.resetLayout(); saved.current = null; dock.current?.clear(); updateEntries(new Map());if(selectedRef.current)autoOpenedEntries.current.delete(selectedRef.current);setNotice('Layout reset');
        }} />;
  return <Context.Provider value={{ bridge, entries, views, selectedWorld, initialStates, liveWorlds: new Set(worlds.filter(world => world.active && world.pendingActive !== false).map(world => world.worldId)) }}><main className="shell">
    <nav className="world-rail" aria-label="Worlds">
      <button className={`home-button rail-button${home ? ' selected' : ''}`} aria-label="Home" title="Home" disabled={!loaded} onClick={() => { setSelectedWorld(null); }}><img src={logo} alt="Rintawa" /></button>
      <div className="rail-divider" />
      <div className="rail-worlds">{worlds.map(world => <button key={world.worldId} title={world.title} aria-label={`Open world ${world.title}`} aria-current={selectedWorld === world.worldId ? 'page' : undefined} disabled={!loaded} className={`world-button rail-button${selectedWorld === world.worldId ? ' selected' : ''}`} onClick={() => selectWorld(world.worldId)}><span>{initials(world.title) || <Icon name="world" />}</span>{world.active && <i className="active-dot" aria-label="Active" />}</button>)}</div>
      <button className="rail-button add-world" title="New World" aria-label="New World" disabled={!loaded || busy} onClick={() => setCreateOpen(true)}><Icon name="plus" size={24} /></button>
      <div className="rail-bottom"><button className="rail-button utility" aria-label="Log out" title="Log out" onClick={() => void bridge.logout().then(() => location.reload())}><Icon name="logout" /></button></div>
    </nav>
    <section className="shell-body">
      {home && <section className="home-page" aria-label="Home"><header className="home-topbar"><span>Home</span><div className="world-actions">{controls}</div></header>
        <Surface bridge={bridge} view={selectedView(views,null,'rintawa.ui.home.renderer')} label="Home"/>
      </section>}
      <section className={`world-workspace${home ? ' hidden-workspace' : ''}`} aria-hidden={home}>
        <header className="world-topbar"><div className="world-title"><Icon name="world" size={18} /><strong>{selected?.title ?? 'World unavailable'}</strong></div><nav className="world-apps" aria-label="World apps">{navigationViews.map(view => <button key={view.id} title={`Open ${view.title}`} onClick={() => open(view, false)}>{view.title}</button>)}</nav><div className="world-actions">{selected && <button className={`world-power${selected.active ? ' running' : ''}`} title={selected.active ? 'Stop World; unfinished work may be discarded' : 'Start World'} disabled={busy || selected.pendingActive !== null} onClick={() => void setWorldActive(selected)}><Icon name="power" size={15} />{selected.pendingActive !== null ? 'Working…' : selected.active ? 'Stop World' : 'Start World'}</button>}{!home && controls}</div></header>
        {selected?.lastError && <Status error>{selected.lastError}</Status>}
        <div className={`dock-area${worldPanels.length <= 1 ? ' single-panel' : ''}`}>{loaded && <DockviewReact components={components} onReady={onReady} theme={theme.mode === 'dark' ? themeDark : themeLight} defaultRenderer="always" disableFloatingGroups getTabContextMenuItems={() => ['close', 'closeOthers']} getTabGroupChipContextMenuItems={() => ['close']} />}</div>
        {!worldPanels.length && !home && <div className="world-welcome"><div className="empty-symbol"><Icon name="world" size={34} /></div><h1>{selected?.title ?? 'World unavailable'}</h1><p>{selected?.active ? 'Choose an app to step into this world.' : 'Start this world to open its chats and characters.'}</p>{selected?.active ? <div className="world-launchers">{available.map(view => <Button key={view.id} title={`Open ${view.title}`} onClick={() => open(view, false)}><Icon name="grid" size={18} /> {view.title}<Icon name="arrow" size={16} /></Button>)}{!available.length && <Status>Waiting for World apps…</Status>}</div> : selected && <Button className="primary" disabled={busy || selected.pendingActive !== null} onClick={() => void setWorldActive(selected)}><Icon name="power" size={16} />{selected.pendingActive !== null ? 'Working…' : 'Start World'}</Button>}</div>}
      </section>
      {(error || readError) && <div className="shell-error" role="alert"><span>{error || readError}</span><button onClick={() => { setError(''); setReadError(''); }} aria-label="Dismiss error"><Icon name="close" size={16} /></button><button onClick={() => void refresh().catch(failure=>setReadError(errorText(failure)))}>Retry reads</button><button onClick={() => location.reload()}>Reconnect</button></div>}
    </section>
    {settings && <div className="settings-overlay"><Dialog open title="Settings" onClose={()=>setSettings(null)}><Surface bridge={bridge} view={selectedView(views,settings.scope,'rintawa.ui.settings.renderer')} label="Settings"/></Dialog></div>}
    {permissionRequest && <Permissions bridge={bridge} source={permissionRequest} scopeTitle={permissionRequest.view.worldId ? worlds.find(w => w.worldId === permissionRequest.view.worldId)?.title ?? permissionRequest.view.worldId : 'Home'} close={closePermissions} />}
    <Dialog open={createOpen} title="Create a world" onClose={closeCreate}><form onSubmit={event => { event.preventDefault(); void createWorld(); }}><label htmlFor="world-template">TEMPLATE</label><input ref={fileInput} className="template-file" type="file" accept=".rtw" hidden disabled={uploadBusy || busy} onChange={event => { const file = event.currentTarget.files?.[0]; event.currentTarget.value = ''; if (file) void importTemplate(file); }} /><select id="world-template" value={template} disabled={busy || uploadBusy} onChange={event => { if (event.target.value === '__file__') fileInput.current?.click(); else setTemplate(event.target.value); }}>{!templates.length && <option value="">No installed templates</option>}{templates.map(row => <option key={row.digest} value={row.digest} disabled={!!row.invalidReason}>{row.name}{row.invalidReason ? ' · unavailable' : ''}</option>)}<option value="__file__">From file (.rtw)…</option></select>{uploadProgress && <Status>{uploadProgress}</Status>}
      {templateInfo && <details className="template-requirements"><summary>Included extensions and permissions</summary>{templateInfo.components.map(slot => <p key={slot.slot}>{slot.packageId ?? slot.slot} {slot.version}</p>)}<ul>{templateInfo.composition.requestedGrants.map(grant => <li key={`${grant.slot}/${grant.componentId}/${grant.permission}`}>{grant.slot}/{grant.componentId}: {grant.permission}{grant.permission==='published-preference-write'&&' (changes shared settings for all worlds)'}</li>)}{creationPermissions.flatMap(p=>[...p.httpOrigins.map(origin=><li key={`${p.slot}/${p.componentId}/http/${origin}`}>{p.slot}/{p.componentId}: connect to {origin}</li>),...p.secretRead.map(path=><li key={`${p.slot}/${p.componentId}/secret/${path}`}>{p.slot}/{p.componentId}: read saved credential {path}</li>)])}</ul></details>}
      {(!!templateInfo?.composition.requestedGrants.length||!!creationPermissions.length) && <label className="template-approval"><input type="checkbox" checked={approved} disabled={busy} onChange={event => setApproved(event.target.checked)} /> Allow the listed permissions in this World</label>}
      <label htmlFor="world-name">WORLD NAME</label><Input id="world-name" value={worldName} onChange={event => setWorldName(event.target.value)} placeholder="Give your world a name" autoFocus disabled={busy} />{error && <Status error>{error}</Status>}<footer><Button disabled={busy} onClick={closeCreate}>Cancel</Button><Button type="submit" className="primary" disabled={busy || uploadBusy || !worldName.trim() || !templateInfo || !accessReady || ((!!templateInfo.composition.requestedGrants.length || !!creationPermissions.length) && !approved)}>Create world</Button></footer></form></Dialog>
  </main></Context.Provider>;
}
