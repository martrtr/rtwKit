import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, BridgeError, type Client } from '@rintawa/bridge';
import { DomainClient, DomainError, type Operation } from '@rintawa/domain';
import { Button, Status } from '@rintawa/ui';
import '@rintawa/ui/style.css';
import './style.css';

type Page<T> = { items: T[]; next: string | null };
type Card = { id: string; card: { name: string } };
type Conversation = { id: string; conversation: { title: string } };
type Subscription = { id: string; revision: string; observerId: string; conversationId: string; sourceFormat: string; lastEntryId: string | null; enabled: boolean; narrationEnabled?: boolean };
type Evidence = { entryId: string; messageId: string; messageRevision: string; role: string };
type RecordSummary = { id: string; revision: string; subscriptionId: string; kind: 'transcript-observation' | 'authored'; status: 'active' | 'suppressed'; evidence: Evidence | null };
type MemoryRecord = RecordSummary & { body: string };
type Unknown = { id: string; kind: 'subscription' | 'record'; expectedRevision?: string; checkpoint?: string | null; reconciled?: boolean };
type ReadStatus<T> = { status: 'pending' } | { status: 'ready'; value: T } | { status: 'failed'; reason: string };
type Clients = { bridge: Client; memory: DomainClient; cards: DomainClient; chats: DomainClient };
const empty = <T,>(): Page<T> => ({ items: [], next: null });
const message = (failure: unknown) => failure instanceof Error ? failure.message : 'Request unavailable';
const code = (failure: unknown) => failure instanceof DomainError || failure instanceof BridgeError ? failure.code : '';
const byteLength = (body: string) => new TextEncoder().encode(body).length;
const maxBody = 16 * 1024;
const short = (id: string) => id.slice(0, 8);
const delay = () => new Promise<void>(resolve => setTimeout(resolve, 40));
const merge = <T extends { id: string }>(old: T[], next: T[]) => [...new Map([...old, ...next].map(item => [item.id, item])).values()];

function App() {
  const client = useRef<Clients | null>(null);
  const mounted = useRef(true);
  const running = useRef(false);
  const abort = useRef<AbortController | null>(null);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [notice, setNotice] = useState('');
  const [unknown, setUnknown] = useState<Unknown | null>(null);
  const [cards, setCards] = useState<Page<Card>>(empty);
  const [chats, setChats] = useState<Page<Conversation>>(empty);
  const [subscriptions, setSubscriptions] = useState<Page<Subscription>>(empty);
  const [observerId, setObserverId] = useState('');
  const [conversationId, setConversationId] = useState('');
  const [selected, setSelected] = useState<Subscription | null>(null);
  const [records, setRecords] = useState<Page<RecordSummary>>(empty);
  const [record, setRecord] = useState<MemoryRecord | null>(null);
  const [draft, setDraft] = useState('');
  const [authoring, setAuthoring] = useState(false);
  const fail = (key: string, error?: unknown) => { if (mounted.current) setErrors(old => ({ ...old, [key]: error == null ? '' : message(error) })); };
  const dispose = () => { client.current?.bridge.dispose(); client.current = null; };
  const check = () => { if (!mounted.current || abort.current?.signal.aborted) throw new DOMException('Panel closed', 'AbortError'); };
  const run = async (action: () => Promise<void>) => {
    if (running.current || !mounted.current || !client.current) return;
    running.current = true; abort.current = new AbortController(); setBusy(true); fail('action');
    try { await action(); }
    catch (failure) { fail('action', failure); }
    finally { running.current = false; abort.current = null; if (mounted.current) setBusy(false); else dispose(); }
  };
  const request = <T,>(domain: DomainClient, method: string, args: Record<string, unknown> = {}) => domain.request<T>(method, args, abort.current?.signal);
  // Panels may mount before service points are registered. Retry startup catalog
  // reads only; explicit authoring always begins exactly once.
  const catalog = async <T,>(domain: DomainClient, method: string, args: Record<string, unknown>): Promise<T> => {
    for (let attempt = 0; ; attempt++) {
      check();
      try { return await request<T>(domain, method, args); }
      catch (failure) { if (attempt >= 4 || !['unavailable', 'limit-exceeded'].includes(code(failure))) throw failure; await delay(); }
    }
  };
  // Record reads are staged because source validation crosses the owner boundary.
  // Keep the bridge alive through a late acknowledgement so even a closed panel releases it.
  const readRecordValue = async <T,>(method: string, args: Record<string, unknown>): Promise<T> => {
    const domain = client.current!.memory;
    const result = await domain.request<{status: string; operation: string; itemIds: string[]}>(method, args);
    if (result.status !== 'pending' || !/^[A-Za-z0-9_-]{43}$/.test(result.operation) || !Array.isArray(result.itemIds) || result.itemIds.length !== 0) throw new DomainError('protocol-error');
    const operation: Operation = { operation: result.operation, itemIds: [] };
    try {
      for (;;) {
        check();
        let status: ReadStatus<T>;
        try { status = await request(domain, 'status', { operation: operation.operation }); }
        catch (failure) { if (!['unavailable', 'limit-exceeded'].includes(code(failure))) throw failure; await delay(); continue; }
        check();
        if (status.status === 'ready' && 'value' in status) return status.value;
        if (status.status === 'failed') throw new Error(status.reason);
        if (status.status !== 'pending') throw new DomainError('protocol-error');
        await delay();
      }
    } finally { await domain.release(operation).catch(() => {}); }
  };
  const readPage = async <T extends {id: string}>(key: string, read: () => Promise<Page<T>>, apply: (page: Page<T>) => void) => {
    try { const page = await read(); check(); apply(page); fail(key); }
    catch (failure) { fail(key, failure); }
  };
  const loadCards = (after?: string) => readPage('Characters', () => catalog<Page<Card>>(client.current!.cards, 'list', after ? { after } : {}), page => setCards(old => ({ ...page, items: after ? merge(old.items, page.items) : page.items })));
  const loadChats = (after?: string) => readPage('Conversations', () => catalog<Page<Conversation>>(client.current!.chats, 'list', after ? { after } : {}), page => setChats(old => ({ ...page, items: after ? merge(old.items, page.items) : page.items })));
  const loadSubscriptions = (after?: string) => readPage('Subscriptions', () => catalog<Page<Subscription>>(client.current!.memory, 'list-subscriptions', after ? { after } : {}), page => setSubscriptions(old => ({ ...page, items: after ? merge(old.items, page.items) : page.items })));
  const loadRecords = (id: string, after?: string) => readPage('Records (last confirmed state)', () => readRecordValue<Page<RecordSummary>>('list-records', { subscriptionId: id, ...(after ? { after } : {}) }), page => setRecords(old => ({ ...page, items: after ? merge(old.items, page.items) : page.items })));
  const loadSubscription = async (id: string) => {
    const value = await request<Subscription>(client.current!.memory, 'get-subscription', { subscriptionId: id });
    check(); setSelected(value); return value;
  };
  const loadRecord = async (id: string) => {
    const value = await readRecordValue<MemoryRecord>('get-record', { recordId: id });
    check(); setRecord(value); setDraft(value.body); setAuthoring(false); fail('Record (last confirmed state)'); return value;
  };
  const openSubscription = async (id: string) => {
    await loadSubscription(id);
    setRecords(empty()); setRecord(null); setAuthoring(false); setDraft(''); fail('Record (last confirmed state)'); fail('Records (last confirmed state)');
    await loadRecords(id);
  };
  const refresh = async () => {
    await loadCards(); check(); await loadChats(); check(); await loadSubscriptions(); check();
    if (selected) { await loadSubscription(selected.id); await loadRecords(selected.id); }
    if (record) { try { await loadRecord(record.id); } catch (failure) { fail('Record (last confirmed state)', failure); } }
  };
  useEffect(() => {
    mounted.current = true;
    connectPanel().then(async bridge => {
      if (!mounted.current) { bridge.dispose(); return; }
      client.current = { bridge, memory: new DomainClient(bridge, 'rintawa/memory/v1'), cards: new DomainClient(bridge, 'rintawa/character/v1'), chats: new DomainClient(bridge, 'rintawa/chat/v1') };
      await run(async () => { setReady(true); await refresh(); });
    }).catch(error => fail('Connection', error));
    return () => { mounted.current = false; abort.current?.abort(); if (!running.current) dispose(); };
  }, []);
  const mutate = async (method: string, args: Record<string, unknown>, capture: Unknown, confirmed: () => Promise<void>) => {
    const domain = client.current!.memory;
    let operation: Operation | undefined; let outcomeKnown = false;
    setNotice('Working…');
    try {
      check(); operation = await domain.begin(method, args); check();
      const result = await domain.wait(operation, abort.current?.signal); check();
      if (result.status === 'unknown') { setUnknown(capture); setNotice('Outcome unknown. Reread the captured item before another mutation.'); return; }
      outcomeKnown = true;
      if (result.status === 'failed') { setNotice(''); throw new Error(result.reason); }
      setNotice('Operation committed.'); await confirmed();
    } catch (failure) {
      const notStarted = failure instanceof DomainError || failure instanceof BridgeError ? failure.outcome === 'not-started' : false;
      if (mounted.current && !outcomeKnown && (operation || !notStarted)) { setUnknown(capture); setNotice('Outcome unknown. Reread the captured item before another mutation.'); }
      else if (!outcomeKnown && mounted.current) setNotice('');
      throw failure;
    } finally { if (operation) await domain.release(operation).catch(() => {}); }
  };
  const reconcile = async (capture: Unknown) => {
    try {
      const value = capture.kind === 'subscription'
        ? await request<Subscription>(client.current!.memory, 'get-subscription', { subscriptionId: capture.id })
        : await readRecordValue<MemoryRecord>('get-record', { recordId: capture.id });
      check();
      const changed = capture.expectedRevision === undefined || value.revision !== capture.expectedRevision || ('lastEntryId' in value && capture.checkpoint !== undefined && value.lastEntryId !== capture.checkpoint);
      if (capture.kind === 'subscription') { setSelected(value as Subscription); await loadRecords(capture.id); await loadSubscriptions(); }
      else { const item = value as MemoryRecord; setRecord(item); setDraft(item.body); setAuthoring(false); await loadRecords(item.subscriptionId); }
      setUnknown({ ...capture, reconciled: true });
      setNotice(changed ? `Item reread at revision ${value.revision}; inspect current state before continuing.` : 'Item still has the captured revision/checkpoint. This does not prove the operation failed.');
    } catch (failure) {
      if (['not-found', 'item-not-found'].includes(code(failure)) || ['not-found', 'item-not-found'].includes(message(failure))) {
        setUnknown({ ...capture, reconciled: true }); setNotice('Item absent on reread. Absence does not prove rollback.');
      } else throw failure;
    }
  };
  const locked = !ready || busy || !!unknown;
  const name = (id: string) => cards.items.find(item => item.id === id)?.card.name ?? id;
  const title = (id: string) => chats.items.find(item => item.id === id)?.conversation.title ?? id;
  const startSubMutation = (method: string, extra = {}) => {
    if (!selected) return;
    const captured = selected;
    void run(() => mutate(method, { subscriptionId: captured.id, expectedRevision: captured.revision, ...extra }, { id: captured.id, kind: 'subscription', expectedRevision: captured.revision, checkpoint: captured.lastEntryId }, async () => { await loadSubscription(captured.id); await loadSubscriptions(); await loadRecords(captured.id); }));
  };
  const updateRecord = (status: RecordSummary['status'], body: string) => {
    if (!record) return;
    const captured = record;
    void run(() => mutate('begin-update-record', { recordId: captured.id, expectedRevision: captured.revision, body, status }, { id: captured.id, kind: 'record', expectedRevision: captured.revision }, async () => { await loadRecord(captured.id); await loadRecords(captured.subscriptionId); }));
  };
  return <main className="memory">
    <header><h1>Memory</h1><Button disabled={!ready || busy} onClick={() => void run(refresh)}>Refresh</Button></header>
    <p className="memory-intro">Author inspector · literal transcript observations and authored recollections. Processing is manual; transcript text is evidence of text, not proof that a character witnessed an event.</p>
    {!ready && <Status>Connecting…</Status>}{busy && <Status>Working…</Status>}{notice && <Status>{notice}</Status>}
    {Object.entries(errors).filter(([, error]) => error).map(([key, error]) => <Status key={key} error>{key}: {error}</Status>)}
    {unknown && <section className="memory-unknown"><h2>Uncertain operation</h2><small>{unknown.kind}: {unknown.id}{unknown.expectedRevision && ` · captured revision ${unknown.expectedRevision}`}{unknown.checkpoint !== undefined && ` · checkpoint ${unknown.checkpoint ?? 'none'}`}</small><div className="actions"><Button disabled={busy} onClick={() => void run(() => reconcile(unknown))}>Reread captured item</Button><Button disabled={busy || !unknown.reconciled} onClick={() => { setUnknown(null); setNotice('Reconciliation reviewed. Only explicit new actions are allowed.'); }}>Acknowledge current state</Button></div></section>}
    <div className="memory-layout">
      <aside><section><h2>New subscription</h2>
        <label>Character card<select disabled={locked} value={observerId} onChange={e => setObserverId(e.target.value)}><option value="">Select character</option>{cards.items.map(item => <option key={item.id} value={item.id}>{item.card.name} · {short(item.id)}</option>)}</select></label>
        {cards.next && <Button disabled={locked} onClick={() => void run(() => loadCards(cards.next!))}>More characters</Button>}
        <label>Conversation<select disabled={locked} value={conversationId} onChange={e => setConversationId(e.target.value)}><option value="">Select conversation</option>{chats.items.map(item => <option key={item.id} value={item.id}>{item.conversation.title} · {short(item.id)}</option>)}</select></label>
        {chats.next && <Button disabled={locked} onClick={() => void run(() => loadChats(chats.next!))}>More conversations</Button>}
        <Button className="primary" disabled={locked || !observerId || !conversationId || !cards.items.some(item => item.id === observerId) || !chats.items.some(item => item.id === conversationId)} onClick={() => { const bound = { observerId, conversationId }; void run(async () => { const [id] = await client.current!.memory.allocate(1); await mutate('begin-create-subscription', { subscriptionId: id, ...bound }, { id, kind: 'subscription' }, async () => { await openSubscription(id); await loadSubscriptions(); }); }); }}>Create subscription</Button>
      </section><section><h2>Subscriptions</h2>{subscriptions.items.length === 0 && <small>No subscriptions loaded.</small>}{subscriptions.items.map(item => <button className={`memory-row${selected?.id === item.id ? ' selected' : ''}`} disabled={busy || !!unknown} key={item.id} onClick={() => void run(() => openSubscription(item.id))}><strong>{name(item.observerId)}</strong><span>{title(item.conversationId)}</span><small>{item.enabled ? 'Enabled' : 'Disabled'} · {short(item.id)}</small></button>)}{subscriptions.next && <Button disabled={locked} onClick={() => void run(() => loadSubscriptions(subscriptions.next!))}>More subscriptions</Button>}</section></aside>
      <div className="memory-detail">{selected ? <><section><h2>{name(selected.observerId)} / {title(selected.conversationId)}</h2><small>Subscription {selected.id} · revision {selected.revision} · {selected.enabled ? 'enabled' : 'disabled'}</small><small>Checkpoint: {selected.lastEntryId ?? 'Not processed'}</small><div className="actions"><Button className="primary" disabled={locked || !selected.enabled} onClick={() => startSubMutation('begin-process')}>Process next page</Button><Button disabled={locked} onClick={() => startSubMutation('begin-set-enabled', { enabled: !selected.enabled })}>{selected.enabled ? 'Disable' : 'Enable'}</Button><Button disabled={locked} onClick={() => startSubMutation('begin-set-narration', { enabled: !selected.narrationEnabled })}>{selected.narrationEnabled ? 'Disable AI memory context' : 'Allow AI memory context'}</Button><small>Opt-in sends eligible recollections to the configured AI provider through Narrator.</small></div></section>
      <section><h2>Records <Button disabled={locked} onClick={() => { setRecord(null); setDraft(''); setAuthoring(true); fail('Record (last confirmed state)'); }}>Add recollection</Button></h2>{records.items.length === 0 && <small>No records loaded.</small>}{records.items.map(item => <button key={item.id} disabled={locked} className={`memory-row${record?.id === item.id ? ' selected' : ''}`} onClick={() => void run(async () => { try { await loadRecord(item.id); } catch (failure) { fail('Record (last confirmed state)', failure); } })}><strong>{item.kind === 'authored' ? 'Authored recollection' : `Transcript · ${item.evidence?.role ?? 'Unknown role'}`}</strong><small>{item.status} · {short(item.id)} · revision {item.revision}</small></button>)}{records.next && <Button disabled={locked} onClick={() => void run(() => loadRecords(selected.id, records.next!))}>More records</Button>}</section>
      {(record || authoring) && <section><h2>{record ? (record.kind === 'authored' ? 'Authored recollection' : 'Transcript observation') : 'Add recollection'} <Button disabled={busy} onClick={() => { setRecord(null); setAuthoring(false); setDraft(''); }}>Close</Button></h2>{record && <small>{record.id} · revision {record.revision} · {record.status}</small>}{record?.evidence && <dl><dt>Original role</dt><dd>{record.evidence.role}</dd><dt>Entry ID</dt><dd>{record.evidence.entryId}</dd><dt>Message ID</dt><dd>{record.evidence.messageId}</dd><dt>Message revision</dt><dd>{record.evidence.messageRevision}</dd></dl>}{record?.kind === 'transcript-observation' && <small>Editing changes the stored observation body, not source evidence.</small>}
        <label>{record ? 'Stored body' : 'Authored body'}<textarea value={draft} disabled={locked} onChange={e => setDraft(e.target.value)} /></label><small>{byteLength(draft)} / {maxBody} UTF-8 bytes</small><div className="actions">{record ? <><Button disabled={locked || draft === record.body || byteLength(draft) > maxBody} onClick={() => updateRecord(record.status, draft)}>Save edit</Button><Button disabled={locked || draft !== record.body} onClick={() => updateRecord(record.status === 'active' ? 'suppressed' : 'active', record.body)}>{record.status === 'active' ? 'Suppress' : 'Restore'}</Button></> : <Button className="primary" disabled={locked || byteLength(draft) > maxBody} onClick={() => { const binding = selected.id; const body = draft; void run(async () => { const [id] = await client.current!.memory.allocate(1); await mutate('begin-create-authored', { subscriptionId: binding, recordId: id, body }, { id, kind: 'record' }, async () => { await loadRecord(id); await loadRecords(binding); }); }); }}>Save recollection</Button>}</div></section>}
      </> : <section><h2>Select a subscription</h2><small>Choose a card and conversation to create a separate memory subscription.</small></section>}</div>
    </div>
  </main>;
}
createRoot(document.getElementById('root')!).render(<App />);
