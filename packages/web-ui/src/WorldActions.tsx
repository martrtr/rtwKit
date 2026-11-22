import { useEffect, useRef, useState } from 'react';
import { BridgeError, handle, type RootBridge, type Theme, type View } from '@rintawa/bridge';
import { Button, Dialog, Icon, Status } from '@rintawa/ui';
import { checkCancelled, closePackage, packageRead, pause, savePackage, waitPackage, type SavePicker } from './packages';
type World = { worldId: string; title: string; active: boolean; pendingActive: boolean | null; lastError: string | null };
type Export = { world: World; phase: 'review' | 'working' | 'ready' | 'saving'; handle?: string; size?: number };
const message = (error: unknown) => error instanceof Error ? error.message : 'Operation unavailable';
const cancelled = (error: unknown) => error instanceof DOMException && error.name === 'AbortError';
export function WorldActions({ bridge, world, views, theme, setTheme, open, reset, refresh, deleted, stopping, fail }: {
  bridge: RootBridge; world: World | null; views: View[]; theme: Theme; setTheme: (theme: Theme) => void;
  open: (view: View) => void; reset: () => Promise<void>; refresh: () => Promise<unknown>;
  deleted: (id: string) => Promise<void>; stopping: (id: string) => void; fail: (error: unknown) => void;
}) {
  const menu = useRef<HTMLDivElement>(null); const trigger = useRef<HTMLButtonElement>(null);
  const [menuOpen, setMenuOpen] = useState(false); const [remove, setRemove] = useState<World | null>(null);
  const [job, setJob] = useState<Export | null>(null); const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const [progress, setProgress] = useState(''); const controller = useRef<AbortController | null>(null); const download = useRef<string | null>(null);
  const hideMenu = () => menu.current?.hidePopover();
  const release = () => { if (download.current) { const id = download.current; download.current = null; void closePackage(bridge.client, id).catch(() => {}); } };
  const dismiss = () => { controller.current?.abort(); release(); setJob(null); setError(''); setProgress(''); };
  useEffect(() => { hideMenu(); }, [world?.worldId]);
  useEffect(() => () => { controller.current?.abort(); release(); }, [bridge]);
  useEffect(() => {
    if (!job?.handle || !['ready', 'saving'].includes(job.phase)) return;
    const timer = setInterval(() => void packageRead(bridge.client, 'packages.status', { handle: job.handle }).catch(failure => setError(message(failure))), 15_000);
    return () => clearInterval(timer);
  }, [bridge, job?.handle, job?.phase]);
  const stop = async (id: string, signal: AbortSignal) => {
    let rows = await packageRead<World[]>(bridge.client, 'world-sessions.list-worlds', {}, signal);
    let row = rows.find(value => value.worldId === id); if (!row) throw new Error('World no longer exists.');
    if (!row.active && row.pendingActive === null) return;
    stopping(id); setProgress('Stopping World…');
    try { await bridge.client.request('world-sessions.set-active', { worldId: id, active: false }); }
    catch (failure) { if (!(failure instanceof BridgeError) || failure.code !== 'unavailable' || failure.outcome !== 'unknown') throw failure; }
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      rows = await packageRead<World[]>(bridge.client, 'world-sessions.list-worlds', {}, signal);
      row = rows.find(value => value.worldId === id); if (!row) throw new Error('World no longer exists.');
      if (!row.active && row.pendingActive === null) { await refresh(); return; }
      if (row.lastError && row.pendingActive === null) throw new Error(row.lastError);
      await pause(signal);
    }
    throw new Error('World stop is not confirmed. Refresh its status and try again.');
  };
  const deleteWorld = async () => {
    if (!remove) return;
    const target = remove; const abort = new AbortController(); controller.current = abort;
    setBusy(true); setError('');
    try {
      await stop(target.worldId, abort.signal); checkCancelled(abort.signal);
      try { await bridge.client.request('world-sessions.delete', { worldId: target.worldId }); }
      catch (failure) {
        // A lost acknowledgement never resubmits deletion.
        const rows = await packageRead<World[]>(bridge.client, 'world-sessions.list-worlds');
        if (rows.some(row => row.worldId === target.worldId)) throw failure;
      }
      await deleted(target.worldId); setRemove(null);
    } catch (failure) { if (!cancelled(failure)) setError(message(failure)); }
    finally { controller.current = null; setBusy(false); setProgress(''); void refresh().catch(fail); }
  };
  const prepareExport = async () => {
    if (!job) return;
    const target = job.world; const abort = new AbortController(); controller.current = abort;
    setBusy(true); setError(''); setJob({ world: target, phase: 'working' });
    let id: string | null = null; let keep = false;
    try {
      await stop(target.worldId, abort.signal); checkCancelled(abort.signal); setProgress('Preparing export…');
      id = handle(await bridge.client.request('world-sessions.begin-export', { worldId: target.worldId })); download.current = id;
      const result = await waitPackage(bridge.client, id, abort.signal);
      if (result.status !== 'download') throw new Error('Invalid World export result');
      checkCancelled(abort.signal); keep = true;
      setJob({ world: target, phase: 'ready', handle: id, size: Number(handle(result.size)) }); setProgress('');
    } catch (failure) { if (!cancelled(failure)) { setError(message(failure)); setJob({ world: target, phase: 'review' }); } }
    finally {
      if (!keep && id) { if (download.current === id) download.current = null; await closePackage(bridge.client, id).catch(() => {}); }
      if (controller.current === abort) controller.current = null;
      setBusy(false); void refresh().catch(fail);
    }
  };
  const downloadExport = async () => {
    if (!job?.handle || job.size === undefined) return;
    const ready = job; const abort = new AbortController(); controller.current = abort;
    const filename = `${ready.world.title.replace(/[/\\\u0000-\u001f:*?"<>|]/g, '_').slice(0, 80) || ready.world.worldId}.rtw`;
    const picker = (window as unknown as { showSaveFilePicker?: SavePicker }).showSaveFilePicker;
    // Invoke the browser picker directly in the user's gesture, after packaging.
    const target = picker ? picker.call(window, { suggestedName: filename, types: [{ description: 'Rintawa World', accept: { 'application/octet-stream': ['.rtw'] } }] }) : Promise.resolve(null);
    setBusy(true); setError(''); setJob({ ...ready, phase: 'saving' }); setProgress('Saving export…');
    try {
      const writer = await target; checkCancelled(abort.signal);
      await savePackage(bridge.client, ready.handle!, ready.size!, filename, writer, abort.signal);
      release(); setJob(null); setProgress('');
    } catch (failure) { if (!abort.signal.aborted) { if (!cancelled(failure)) setError(message(failure)); setJob(ready); setProgress(''); } }
    finally { if (controller.current === abort) controller.current = null; setBusy(false); }
  };
  const menuAction = (work: () => void | Promise<void>) => { hideMenu(); try { void Promise.resolve(work()).catch(fail); } catch (failure) { fail(failure); } };
  return <>
    <button ref={trigger} className="icon-button" title="Workspace controls" aria-label="Workspace controls" aria-haspopup="menu" aria-controls="workspace-menu" aria-expanded={menuOpen} popoverTarget="workspace-menu" disabled={!world}><Icon name="more" /></button>
    <div ref={menu} id="workspace-menu" className="workspace-menu" popover="auto" role="menu" onToggle={event => { const opened = (event.nativeEvent as ToggleEvent).newState === 'open'; setMenuOpen(opened); if (opened) menu.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus(); }} onKeyDown={event => {
      const items = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) { event.preventDefault(); items[event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowUp' ? -1 : 1) + items.length) % items.length]?.focus(); }
      if (event.key === 'Escape') { hideMenu(); trigger.current?.focus(); }
    }}>
      <button role="menuitem" disabled={!world} onClick={() => menuAction(() => { setError(''); setJob({ world: world!, phase: 'review' }); })}>Export World…</button>
      <button role="menuitem" className="danger" disabled={!world} onClick={() => menuAction(() => { setError(''); setRemove(world); })}>Delete World…</button>
      <div className="menu-divider" role="separator" />
      {views.map(view => <button key={view.id} role="menuitem" onClick={() => menuAction(() => open(view))}><Icon name="split" size={15} />Split {view.title}</button>)}
      <button role="menuitem" onClick={() => menuAction(() => setTheme({ mode: theme.mode === 'dark' ? 'light' : 'dark' }))}>{theme.mode === 'dark' ? 'Light theme' : 'Dark theme'}</button>
      <button role="menuitem" onClick={() => menuAction(reset)}>Reset layout</button>
    </div>
    <Dialog open={!!remove} title="Delete World" onClose={() => { if (!busy) { setRemove(null); setError(''); } }}>
      <p>Delete <strong>{remove?.title}</strong> and all of its saved data? This cannot be undone.</p>
      {(remove?.active || remove?.pendingActive !== null) && <p>The World will be stopped first.</p>}
      {progress && <Status>{progress}</Status>}{error && <Status error>{error}</Status>}
      <footer><Button disabled={busy} onClick={() => { setRemove(null); setError(''); }}>Cancel</Button><Button className="danger" disabled={busy} onClick={() => void deleteWorld()}>Delete World</Button></footer>
    </Dialog>
    <Dialog open={!!job} title="Export World" onClose={dismiss}>
      <p><strong>{job?.world.title}</strong></p>
      {job?.phase === 'review' && <p>The export includes saved chats, Characters and their assets. Local credentials are excluded. The World will be stopped.</p>}
      {job?.phase === 'ready' && <Status>Export ready · {Math.ceil((job.size ?? 0) / 1024)} KiB</Status>}
      {progress && <Status>{progress}</Status>}{error && <Status error>{error}</Status>}
      <footer><Button onClick={dismiss}>Cancel</Button>{job?.phase === 'review' && <Button className="primary" disabled={busy} onClick={() => void prepareExport()}>Create export</Button>}{job?.phase === 'ready' && <Button className="primary" disabled={busy} onClick={() => void downloadExport()}>Download .rtw</Button>}</footer>
    </Dialog>
  </>;
}
