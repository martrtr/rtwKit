import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, BridgeError, type Client } from '@rintawa/bridge';
import { DomainClient, DomainError, observeResource, type Observation, type Operation } from '@rintawa/domain';
import { Button, Input, Status } from '@rintawa/ui';
import { discover, search, labels, type Catalog, type Item } from './catalogs';
import '@rintawa/ui/style.css';
import './style.css';
type Scene = { revision: string; title: string; description: string };
type Summary = { id: string; title: string; revision: string };
type Member = { id: string; itemId: string };
const failure = (e: unknown) => e instanceof Error ? e.message : 'Unavailable';
function App() {
  const client = useRef<Client | null>(null), domain = useRef<DomainClient | null>(null), alive = useRef(false);
  const selectedRef = useRef<string | null>(null), dirty = useRef(false), pages = useRef(1), listPages = useRef(1);
  const draftRevision = useRef<string | null>(null);
  const work = useRef<Operation | null>(null), controller = useRef<AbortController | null>(null), target = useRef<string | null>(null);
  const listWatch = useRef<Observation | null>(null), sceneWatch = useRef<Observation | null>(null);
  const [ready, setReady] = useState(false), [rows, setRows] = useState<Summary[]>([]), [listNext, setListNext] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null), [scene, setScene] = useState<Scene | null>(null);
  const [title, setTitle] = useState(''), [description, setDescription] = useState(''), [changed, setChanged] = useState(false);
  const [members, setMembers] = useState<Member[]>([]), [next, setNext] = useState<string | null>(null), [names, setNames] = useState<Map<string, string>>(new Map());
  const [busy, setBusy] = useState(false), [unknown, setUnknown] = useState(false), [error, setError] = useState(''), [notice, setNotice] = useState('');
  const [catalogs, setCatalogs] = useState<Catalog[]>([]), [catalogKey, setCatalogKey] = useState(''), [query, setQuery] = useState('');
  const [results, setResults] = useState<Item[]>([]), [searchAfter, setSearchAfter] = useState<string | null>(null), [searchNext, setSearchNext] = useState<string | null>(null);
  const [catalogError, setCatalogError] = useState(''), [searching, setSearching] = useState(false), [manual, setManual] = useState('');
  useEffect(() => {
    alive.current = true; let catalogWatch: Observation | undefined;
    void connectPanel().then(c => {
      if (!alive.current) { c.dispose(); return; }
      client.current = c; domain.current = new DomainClient(c, 'rintawa/scenes/v2'); setReady(true);
      listWatch.current = observeResource({ client: c, points: ['rintawa/scenes/changed/v2'], read: async signal => {
        let after: string | null = null; const items: Summary[] = [];
        for (let n = 0; n < listPages.current; n++) {
          const p: { items: Summary[]; next: string | null } = await domain.current!.request('list-scenes', { after }, signal);
          items.push(...p.items); after = p.next; if (!after) break;
        } return { items, next: after };
      }, apply: v => { setRows(v.items); setListNext(v.next); }, failed: e => setError(failure(e)) });
      catalogWatch = observeResource({ client: c, read: signal => discover(c, signal), apply: v => {
        setCatalogs(old => JSON.stringify(old) === JSON.stringify(v.items) ? old : v.items);
        if (v.rejected) setCatalogError('Some catalogs returned invalid descriptions.');
      }, failed: e => setCatalogError(failure(e)) });
    }).catch(e => { if (alive.current) setError(failure(e)); });
    return () => { alive.current = false; controller.current?.abort(); listWatch.current?.close(); sceneWatch.current?.close(); catalogWatch?.close(); if (work.current) void domain.current?.release(work.current).catch(() => {}); client.current?.dispose(); };
  }, []);
  useEffect(() => {
    if (!selected || !client.current) return; const id = selected;
    const watch = observeResource({ client: client.current, points: ['rintawa/scenes/changed/v2'], resource: id, read: async signal => {
      const v = await domain.current!.request<{ id: string; scene: Scene }>('get-scene', { sceneId: id }, signal);
      let after: string | null = null; const items: Member[] = [];
      for (let n = 0; n < pages.current; n++) {
        const p: { items: Member[]; next: string | null } = await domain.current!.request('list-members', { sceneId: id, expectedRevision: v.scene.revision, after }, signal);
        items.push(...p.items); after = p.next; if (!after) break;
      }
      const current = await domain.current!.request<{ scene: Scene }>('get-scene', { sceneId: id }, signal);
      if (current.scene.revision !== v.scene.revision) throw Error('Scene changed while reading; waiting for current context.');
      return { scene: v.scene, items, next: after };
    }, apply: v => {
      if (selectedRef.current !== id) return;
      setScene(v.scene); setMembers(old => JSON.stringify(old) === JSON.stringify(v.items) ? old : v.items); setNext(v.next);
      if (!dirty.current) { setTitle(v.scene.title); setDescription(v.scene.description); setChanged(false); }
    }, failed: e => setError(failure(e)) });
    sceneWatch.current = watch; return () => { watch.close(); if (sceneWatch.current === watch) sceneWatch.current = null; };
  }, [selected]);
  useEffect(() => {
    if (!client.current || !members.length) { setNames(new Map()); return; }
    const c = client.current;
    const watch = observeResource({client:c,read:async signal=>{
      const all = new Map<string, string>();
      for (let i = 0; i < members.length; i += 32) for (const [id, label] of await labels(c, members.slice(i, i + 32).map(m => m.itemId), signal)) all.set(id, label);
      return all;
    },apply:setNames,failed:()=>{}});
    return ()=>watch.close();
  }, [members, catalogs]);
  const catalog = catalogs.find(c => c.key === catalogKey) ?? (!catalogKey ? catalogs[0] : undefined);
  useEffect(() => {
    if (!ready || !client.current || !catalog) { setResults([]); setSearchNext(null); setSearching(false); return; }
    const abort = new AbortController(), c = client.current; setSearching(true);
    const timer = setTimeout(() => {
      void search(c, catalog, query, searchAfter, abort.signal).then(v => {
        if (!abort.signal.aborted) { setResults(v.items); setSearchNext(v.next); setCatalogError(''); }
      }).catch(e => { if (!abort.signal.aborted) { setResults([]); setSearchNext(null); setCatalogError(failure(e)); } })
        .finally(() => { if (!abort.signal.aborted) setSearching(false); });
    }, 200);
    return () => { clearTimeout(timer); abort.abort(); };
  }, [ready, catalog?.key, query, searchAfter]);
  const select = (id: string | null) => {
    selectedRef.current = id; dirty.current = false; pages.current = 1;
    setSelected(id); setScene(null); setMembers([]); setNext(null); setTitle(''); setDescription(''); setChanged(false); setError(''); setNotice('');
  };
  const run = async (method: string, args: Record<string, unknown>, id: string) => {
    if (!domain.current) return; setBusy(true); setError(''); setNotice(''); target.current = id;
    const stop = new AbortController(); controller.current = stop;
    let settled = false;
    try {
      const op = await domain.current.begin(method, args); work.current = op;
      if (!alive.current) { void domain.current.release(op).catch(() => {}); return; }
      const result = await domain.current.wait(op, stop.signal); work.current = null; if (!alive.current) return;
      settled = result.status !== 'unknown';
      if (result.status === 'unknown') { setUnknown(true); return; }
      if (result.status === 'failed') { setError(result.reason); if (result.reason.includes('stale-context')) await sceneWatch.current?.refresh(); return; }
      dirty.current = false; setChanged(false); if (!selected) select(id); else await sceneWatch.current?.refresh();
      await listWatch.current?.refresh(); setNotice('Context saved.');
    } catch (e) {
      if (alive.current) { setError(failure(e)); if (!settled && !((e instanceof DomainError || e instanceof BridgeError) && e.outcome === 'not-started')) setUnknown(true); }
    } finally { controller.current = null; if (alive.current) setBusy(false); }
  };
  const save = async () => {
    if (!domain.current) return; setBusy(true);
    try { const id = selected ?? (await domain.current.allocate(1))[0]; await run(selected ? 'begin-update-scene' : 'begin-create-scene', selected ? { sceneId: id, expectedRevision: draftRevision.current ?? scene!.revision, title, description } : { sceneId: id, title, description }, id); }
    catch (e) { setError(failure(e)); } finally { if (alive.current) setBusy(false); }
  };
  const membership = async (itemId: string, add: boolean) => {
    if (!selected || !scene) return;
    await run(add ? 'begin-add-member' : 'begin-remove-member', { sceneId: selected, expectedRevision: scene.revision, itemId }, selected);
  };
  const reconcile = async () => {
    setBusy(true);
    try {
      if (work.current) { const status = await domain.current!.status(work.current); if (status.status === 'pending') { setNotice('The original operation is still running.'); return; } await domain.current!.release(work.current); work.current = null; }
      if (target.current) { const v = await domain.current!.request<{ scene: Scene }>('get-scene', { sceneId: target.current }); if (!selected) select(target.current); setScene(v.scene); }
      await sceneWatch.current?.refresh(); await listWatch.current?.refresh(); setUnknown(false);
      setNotice('Current state reloaded. Nothing was repeated; an unacknowledged original operation may still complete.');
    } catch (e) { setError(failure(e)); } finally { setBusy(false); }
  };
  const locked = busy || unknown || !ready, memberIds = new Set(members.map(m => m.itemId));
  return <main className="scenes"><aside><h1>Scenes</h1><p>Saved contexts for any World items.</p>
    <Button disabled={locked} onClick={() => select(null)}>New scene</Button><nav>{rows.map(r => <Button key={r.id} disabled={locked} aria-label={r.title} aria-current={selected === r.id ? 'page' : undefined} onClick={() => select(r.id)}>{r.title}</Button>)}</nav>
    {listNext && <Button disabled={locked} onClick={() => { listPages.current++; void listWatch.current?.refresh(); }}>More scenes</Button>}</aside>
    <section><h2>{selected ? scene?.title ?? 'Loading scene…' : 'New scene'}</h2>
      <label>Scene name<Input aria-label="Scene name" disabled={locked || !!selected && !scene} value={title} onChange={e => { if (!dirty.current) draftRevision.current = scene?.revision ?? null; dirty.current = true; setChanged(true); setTitle(e.target.value); }} /></label>
      <label>Context description<textarea aria-label="Scene description" disabled={locked || !!selected && !scene} value={description} onChange={e => { if (!dirty.current) draftRevision.current = scene?.revision ?? null; dirty.current = true; setChanged(true); setDescription(e.target.value); }} /></label>
      <Button disabled={locked || !!selected && !scene || !title.trim() || new TextEncoder().encode(title).length > 256 || new TextEncoder().encode(description).length > 4096 || !!selected && !changed} onClick={() => void save()}>Save scene</Button>
      {changed && selected && <Button disabled={locked} onClick={() => { dirty.current = false; setChanged(false); setTitle(scene!.title); setDescription(scene!.description); }}>Discard draft</Button>}
      {selected && scene && <><h3>In this scene</h3><p>Current context items. Their extensions define their meaning.</p>
        <div className="member-list">{members.map(m => <div className="scene-member" key={m.id}><span>{names.get(m.itemId) ?? `Item ${m.itemId}`}<small>{names.has(m.itemId) ? 'World item' : 'Description unavailable'}</small></span><Button disabled={locked || changed} onClick={() => void membership(m.itemId, false)}>Remove</Button></div>)}{!members.length && <p>No items in this scene yet.</p>}</div>
        {next && <Button disabled={locked} onClick={() => { pages.current++; void sceneWatch.current?.refresh(); }}>More items</Button>}
        <h3>Add to this scene</h3><label>Catalog<select aria-label="Item catalog" disabled={locked} value={catalog?.key ?? ''} onChange={e => { setCatalogKey(e.target.value); setSearchAfter(null); }}>{catalogs.map(c => <option key={c.key} value={c.key}>{c.label}</option>)}</select></label>
        {!catalogs.length && <p>No item catalogs are available. Existing scene items are kept.</p>}
        <Input aria-label="Search context items" placeholder="Search this catalog…" disabled={locked || !catalog} value={query} onChange={e => { setQuery(e.target.value); setSearchAfter(null); }} />
        {searching && <p>Searching…</p>}<div className="member-list">{results.map(r => <div className="scene-member" key={r.itemId}><span>{r.label}{r.detail && <small>{r.detail}</small>}</span><Button disabled={locked || changed || memberIds.has(r.itemId) || searching} onClick={() => void membership(r.itemId, true)}>{memberIds.has(r.itemId) ? 'Added' : 'Add'}</Button></div>)}</div>
        {searchNext && <Button disabled={locked || searching} onClick={() => setSearchAfter(searchNext)}>Next results</Button>}
        <Status error>{catalogError}</Status><details><summary>Add an item by ID</summary><p>Advanced fallback for items without a catalog.</p><Input aria-label="World item ID" value={manual} onChange={e => setManual(e.target.value)} /><Button disabled={locked || changed || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(manual)} onClick={() => void membership(manual, true)}>Add item</Button></details>
      </>}
      <Status error>{error}</Status><Status>{notice}</Status>{unknown && <div role="alert"><p>The outcome is unknown. Nothing will be repeated automatically.</p><Button disabled={busy} onClick={() => void reconcile()}>Reload current state</Button></div>}
    </section></main>;
}
createRoot(document.getElementById('root')!).render(<App />);
