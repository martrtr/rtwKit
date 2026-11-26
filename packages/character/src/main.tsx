import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, BridgeError, type AssetRef, type Client, type ResourceRef } from '@rintawa/bridge';
import { DomainClient, DomainError, observeResource, type Observation, type Operation } from '@rintawa/domain';
import { Button, Status } from '@rintawa/ui';
import '@rintawa/ui/style.css';
import './style.css';
import { Story } from './Story';
import { PersonaEditor } from './PersonaEditor';

type Character = { id: string; card: { name: string; source: AssetRef; portrait: AssetRef | null } };
type List = { items: Character[]; next: string | null };
const message = (error: unknown) => error instanceof Error ? error.message : 'Request unavailable';

let portraitQueue: Promise<void> = Promise.resolve();
function Portrait({ client, item, large = false }: { client: Client | null; item: Character; large?: boolean }) {
  const [url, setUrl] = useState<string | null>(null);
  const reference = item.card.portrait;
  useEffect(() => {
    let cancelled = false; let revoke: (() => void) | undefined;
    setUrl(null);
    if (client && reference) portraitQueue = portraitQueue.then(async () => {
      if (cancelled) return;
      const image = await client.image(reference);
      if (cancelled) image.revoke(); else { revoke = image.revoke; setUrl(image.url); }
    }).catch(() => {});
    return () => { cancelled = true; revoke?.(); };
  }, [client, reference?.digest, reference?.size, reference?.mediaType]);
  return <span className={large ? 'character-avatar large' : 'character-avatar'} aria-hidden="true">{url ? <img src={url} alt="" onError={() => setUrl(null)} /> : <span>{(Array.from(item.card.name.trim())[0] ?? '').toUpperCase() || '?'}</span>}</span>;
}
function sourceFormat(reference: AssetRef) {
  if (reference.mediaType === 'image/png') return 'PNG';
  if (reference.mediaType === 'application/json') return 'JSON';
  return reference.mediaType;
}
function sourceSize(size: string) {
  try {
    const bytes = BigInt(size);
    if (bytes < 1024n) return `${bytes} B`;
    if (bytes < 1024n * 1024n) return `${Number(bytes * 10n / 1024n) / 10} KiB`;
    return `${Number(bytes * 10n / (1024n * 1024n)) / 10} MiB`;
  } catch { return size; }
}

function App() {
  const bridge = useRef<Client | null>(null); const domain = useRef<DomainClient | null>(null);
  const observation = useRef<Observation | null>(null); const pages = useRef(1);
  const mounted = useRef(true); const resource = useRef<ResourceRef | null>(null);
  const active = useRef<Operation | null>(null); const abort = useRef<AbortController | null>(null);
  const [workflow, setWorkflow] = useState(false);
  const [ready, setReady] = useState(false); const [busy, setBusy] = useState(false);
  const [catalogError, setCatalogError] = useState('');
  const [error, setError] = useState(''); const [notice, setNotice] = useState('');
  const [unknown, setUnknown] = useState<string | null>(null); const [items, setItems] = useState<Character[]>([]);
  const [cursor, setCursor] = useState<string | null>(null); const [selected, setSelected] = useState<Character | null>(null);
  const releaseActive = () => { if (active.current) { void domain.current?.release(active.current).catch(() => {}); active.current = null; } };
  const discard = (ref: ResourceRef) => { void domain.current?.request('discard-resource', { resource: ref }).catch(() => {}); };
  useEffect(() => {
    mounted.current = true;
    connectPanel().then(async client => {
      if (!mounted.current) { client.dispose(); return; }
      bridge.current = client; domain.current = new DomainClient(client, 'rintawa/character/v1');
      try {
        const selected = await domain.current.request<boolean>('workflow');
        if (!mounted.current) return;
        setWorkflow(selected);
        if (!selected) {
          const source = domain.current;
          observation.current = observeResource<List>({
            client, points: ['rintawa/character/changed/v1'],
            read: async signal => {
              const items: Character[] = []; let next: string | null = null;
              for (let page = 0; page < pages.current; page++) {
                const result: List = await source.request<List>('list', next ? { after: next } : {}, signal);
                items.push(...result.items); next = result.next;
                if (!next) break;
              }
              return { items, next };
            },
            apply: result => {
              setItems(result.items); setCursor(result.next); setCatalogError('');
              setSelected(previous => previous ? result.items.find(item => item.id === previous.id) ?? previous : null);
            },
            failed: failure => setCatalogError(`Characters may be stale: ${message(failure)}`),
          });
          await observation.current.refresh();
        }
      } catch (failure) { if (mounted.current && !observation.current) setError(message(failure)); }
      finally { if (mounted.current) setReady(true); }
    }).catch(failure => { if (mounted.current) setError(message(failure)); });
    return () => {
      mounted.current = false; observation.current?.close(); abort.current?.abort();
      if (active.current) void domain.current?.release(active.current).catch(() => {});
      if (resource.current) discard(resource.current);
      // Story owns child subscriptions; let its effect cleanup close them before disposal.
      const client = bridge.current; queueMicrotask(() => client?.dispose());
    };
  }, []);
  const read = async (action: () => Promise<void>) => {
    setBusy(true); setError('');
    try { await action(); } catch (failure) { if (mounted.current) setError(message(failure)); }
    finally { if (mounted.current) setBusy(false); }
  };
  const importFile = async (file: File) => {
    const extension = file.name.toLowerCase().split('.').pop();
    const limit = extension === 'json' ? 4 * 1024 * 1024 : extension === 'png' ? 16 * 1024 * 1024 : 0;
    if (!limit || file.size > limit || !file.size) { setError('Choose a nonempty JSON file up to 4 MiB or PNG up to 16 MiB.'); return; }
    if (!bridge.current || !domain.current) return;
    setBusy(true); setError(''); setNotice('Uploading…'); setUnknown(null);
    const controller = new AbortController(); abort.current = controller;
    let id: string | null = null; let beginSent = false; let committed = false;
    try {
      const ref = await bridge.current.upload(file, file.name); resource.current = ref;
      if (!mounted.current || controller.signal.aborted) { discard(ref); resource.current = null; return; }
      [id] = await domain.current.allocate(1);
      if (!mounted.current || controller.signal.aborted) return;
      beginSent = true; setNotice('Importing…');
      const operation = await domain.current.begin('begin-import', { itemId: id, resource: ref });
      if (!mounted.current || controller.signal.aborted) { void domain.current.release(operation).catch(() => {}); return; }
      active.current = operation;
      const result = await domain.current.wait(operation, controller.signal); active.current = null;
      if (!mounted.current) return;
      if (result.status === 'failed') { setError(result.reason); setNotice(''); }
      else if (result.status === 'unknown') { setUnknown(id); setNotice('Import outcome unknown. Reread this ID before trying again.'); }
      else {
        committed = true; setNotice('Character imported.');
        const card = await domain.current.request<Character>('get', { itemId: id });
        if (mounted.current) setSelected(card);
        await observation.current?.refresh().catch(() => {});
      }
    } catch (failure) {
      if (mounted.current) {
        const notStarted = (failure instanceof DomainError || failure instanceof BridgeError) && failure.outcome === 'not-started';
        if (id && beginSent && !committed && (active.current || !notStarted)) { setUnknown(id); setNotice('Import outcome unknown. Reread this ID before trying again.'); }
        else setNotice('');
        setError(message(failure));
      }
    } finally {
      if (resource.current) { discard(resource.current); resource.current = null; }
      abort.current = null; if (mounted.current) setBusy(false);
    }
  };
  if (workflow && bridge.current) return <Story client={bridge.current} />;
  const importDisabled = !ready || busy || !!unknown;
  return <main className="character characters">
    <header className="character-header">
      <div className="character-heading"><h1>Characters</h1><span className="character-count">{items.length}{cursor ? '+' : ''}</span><span className="character-subtitle">Tavern V2 / V3 · PNG or JSON</span></div>
      <div className="character-actions">
        <label htmlFor="character-file" className={`import-button${importDisabled ? ' disabled' : ''}`} aria-disabled={importDisabled}>Import character</label>
        <input id="character-file" className="file-input" type="file" accept=".png,.json" disabled={importDisabled} onChange={event => { const file = event.currentTarget.files?.[0]; event.currentTarget.value = ''; if (file) void importFile(file); }} />
      </div>
    </header>
    <div className="character-body">
      {!ready && !error && <Status>Connecting…</Status>}{busy && <Status>Working…</Status>}{error && <Status error>{error}</Status>}{notice && <Status>{notice}</Status>}
      {catalogError && <><Status error>{catalogError}</Status><Button disabled={busy} onClick={() => void read(async () => { await observation.current?.refresh().catch(() => {}); })}>Retry</Button></>}
      {unknown && <div className="character-unknown"><p>Known character ID: {unknown}</p><div><Button disabled={busy} onClick={() => void read(async () => { const card = await domain.current!.request<Character>('get', { itemId: unknown }); if (mounted.current) { setSelected(card); releaseActive(); setUnknown(null); setNotice('Character found.'); } await observation.current?.refresh().catch(() => {}); })}>Reread character</Button><Button disabled={busy} onClick={() => { releaseActive(); setUnknown(null); setNotice('An absent item does not prove rollback. Retry only by an explicit decision.'); }}>Allow another import</Button></div></div>}
      {ready && !items.length && !error && !catalogError && !busy && !unknown && <div className="character-empty">
        <svg viewBox="0 0 64 64" fill="none" aria-hidden="true"><rect x="10" y="8" width="44" height="48" rx="6" /><circle cx="32" cy="26" r="7" /><path d="M20 47c1.8-6.2 6-9 12-9s10.2 2.8 12 9" /></svg>
        <h2>No characters yet</h2><p>Import a character card to add it to this World.</p><label htmlFor="character-file" className={`import-button${importDisabled ? ' disabled' : ''}`} aria-disabled={importDisabled}>Import character</label>
      </div>}
      {ready && bridge.current && <PersonaEditor client={bridge.current} busy={busy} />}
      <div className="character-content">
        {!!items.length && <><div className="collection-label">YOUR CHARACTERS <span>{items.length}{cursor ? '+' : ''}</span></div><nav className="character-grid" aria-label="Characters">{items.map(item => <button className={selected?.id === item.id ? 'character-card selected' : 'character-card'} key={item.id} disabled={busy} onClick={() => setSelected(item)}><Portrait client={bridge.current} item={item} /><span className="character-card-text"><strong>{item.card.name}</strong></span><span className="character-arrow" aria-hidden="true" /></button>)}</nav>{cursor && <Button disabled={busy || !!catalogError} onClick={() => void read(async () => { pages.current++; await observation.current?.refresh().catch(() => {}); })}>More characters</Button>}</>}
        <section className={selected ? 'character-detail' : 'character-detail empty'} aria-label="Character details">{selected && <><Portrait client={bridge.current} item={selected} large /><div className="character-detail-copy"><span className="detail-label">CHARACTER</span><h2>{selected.card.name}</h2><p>{sourceFormat(selected.card.source)} · {sourceSize(selected.card.source.size)}</p></div><Button disabled={busy} onClick={() => void read(async () => {
          const operation = await domain.current!.begin('begin-extract-greetings', { itemId: selected.id });
          active.current = operation; const result = await domain.current!.wait(operation); active.current = null;
          if (result.status === 'failed') throw new Error(result.reason); if (result.status === 'unknown') throw new Error('Outcome unknown; reread the greeting list before retrying.');
          setNotice('Greeting choices indexed.');
        })}>Index alternate greetings</Button><button className="close-detail" aria-label="Close character details" onClick={() => setSelected(null)}>×</button></>}</section>
      </div>
    </div>
  </main>;
}
createRoot(document.getElementById('root')!).render(<App />);
