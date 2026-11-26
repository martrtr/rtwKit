import { ExtractionControls, type ExtractionState } from './ExtractionControls';
import { AutomaticControls, type AutomaticStatus } from './AutomaticControls';
import { CycleControls, type CycleState } from './CycleControls';
import { AffectControls, AppraisalControls, type Affect, type Appraisal, type Deltas } from './AffectControls';
import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, BridgeError, type Client } from '@rintawa/bridge';
import { DomainClient, DomainError, type Operation } from '@rintawa/domain';
import { Button, Status } from '@rintawa/ui';
import '@rintawa/ui/style.css';
import './style.css';

type Page<T> = { items: T[]; next: string | null };
type Card = { id: string; card: { name: string } };
type Conversation = { id: string; conversation: { title: string; characterId?: string | null } };
type Subscription = { id: string; revision: string; observerId: string; conversationId: string; lastEntryId: string | null; enabled: boolean; narrationEnabled?: boolean; automaticEnabled?: boolean };
type Evidence = { entryId: string; messageId: string; messageRevision: string; role: string };
type RecordSummary = { id: string; revision: string; subscriptionId: string; kind: 'transcript-observation' | 'authored' | 'extracted'; authorEdited?: boolean; provenance?: { processor: string; evidence: Evidence[] }; status: 'active' | 'suppressed'; evidence: Evidence | null };
type MemoryRecord = RecordSummary & { body: string };
type Unknown = { id: string; kind: 'subscription' | 'record'; expectedRevision?: string; checkpoint?: string | null; reconciled?: boolean };
type ReadStatus<T> = { status: 'pending' } | { status: 'ready'; value: T } | { status: 'failed'; reason: string };
type Clients = { bridge: Client; memory: DomainClient; cards: DomainClient; chats: DomainClient };
const empty = <T,>(): Page<T> => ({ items: [], next: null });
const message = (failure: unknown) => failure instanceof Error ? failure.message : 'Request unavailable';
const code = (failure: unknown) => failure instanceof DomainError || failure instanceof BridgeError ? failure.code : '';
const byteLength = (body: string) => new TextEncoder().encode(body).length;
const maxBody = 16 * 1024;
const delay = () => new Promise<void>(resolve => setTimeout(resolve, 40));
const merge = <T extends { id: string }>(old: T[], next: T[]) => [...new Map([...old, ...next].map(item => [item.id, item])).values()];

function App() {
  const client = useRef<Clients | null>(null);
  const mounted = useRef(true), running = useRef(false), editing = useRef(false);
  const abort = useRef<AbortController | null>(null);
  const [ready, setReady] = useState(false), [busy, setBusy] = useState(false);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [notice, setNotice] = useState('');
  const [unknown, setUnknown] = useState<Unknown | null>(null);
  const [cards, setCards] = useState<Page<Card>>(empty);
  const [chats, setChats] = useState<Page<Conversation>>(empty);
  const [subscriptions, setSubscriptions] = useState<Page<Subscription>>(empty);
  const [observerId, setObserverId] = useState(''), [conversationId, setConversationId] = useState('');
  const [selected, setSelected] = useState<Subscription | null>(null);
  const [records, setRecords] = useState<Page<RecordSummary>>(empty);
  const [bodies, setBodies] = useState<Record<string, MemoryRecord>>({});
  const [record, setRecord] = useState<MemoryRecord | null>(null);
  const [draft, setDraft] = useState('');
  const [automaticRuntime, setAutomaticRuntime] = useState<AutomaticStatus | null>(null);
  const [extraction, setExtraction] = useState<ExtractionState | null>(null);
  const [cycle, setCycle] = useState<CycleState | null>(null);
  const [affect, setAffect] = useState<Affect | null>(null);
  const [appraisal, setAppraisal] = useState<Appraisal | null>(null);
  const [authoring, setAuthoring] = useState(false), [editMode, setEditMode] = useState(false);
  editing.current = authoring || editMode || !!unknown;
  const fail = (key: string, error?: unknown) => { if (mounted.current) setErrors(old => ({ ...old, [key]: error == null ? '' : message(error) })); };
  const dispose = () => { client.current?.bridge.dispose(); client.current = null; };
  const check = () => { if (!mounted.current || abort.current?.signal.aborted) throw new DOMException('Panel closed', 'AbortError'); };
  const run = async (action: () => Promise<void>) => {
    if (running.current || !mounted.current || !client.current) return;
    running.current = true; abort.current = new AbortController(); setBusy(true); fail('action');
    try { await action(); } catch (failure) { fail('action', failure); }
    finally { running.current = false; abort.current = null; if (mounted.current) setBusy(false); else dispose(); }
  };
  const request = <T,>(domain: DomainClient, method: string, args: Record<string, unknown> = {}) => domain.request<T>(method, args, abort.current?.signal);
  const catalog = async <T,>(domain: DomainClient, method: string, args: Record<string, unknown>): Promise<T> => {
    for (let attempt = 0; ; attempt++) {
      check();
      try { return await request<T>(domain, method, args); }
      catch (failure) { if (attempt >= 4 || !['unavailable', 'limit-exceeded'].includes(code(failure))) throw failure; await delay(); }
    }
  };
  // Admission may finish after closing the panel; always release its handle.
  const readRecordValue = async <T,>(method: string, args: Record<string, unknown>): Promise<T> => {
    const domain = client.current!.memory;
    const result = await domain.request<{ status: string; operation: string; itemIds: string[] }>(method, args);
    if (result.status !== 'pending' || !/^[A-Za-z0-9_-]{43}$/.test(result.operation) || !Array.isArray(result.itemIds) || result.itemIds.length !== 0) throw new DomainError('protocol-error');
    const operation: Operation = { operation: result.operation, itemIds: [] };
    try {
      for (;;) {
        check(); let status: ReadStatus<T>;
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
  const loadCards = async (after?: string) => {
    const page = await catalog<Page<Card>>(client.current!.cards, 'list', after ? { after } : {}); check();
    setCards(old => ({ ...page, items: after ? merge(old.items, page.items) : page.items })); return page;
  };
  const loadChats = async (after?: string) => {
    const page = await catalog<Page<Conversation>>(client.current!.chats, 'list', after ? { after } : {}); check();
    setChats(old => ({ ...page, items: after ? merge(old.items, page.items) : page.items })); return page;
  };
  const loadSubscriptions = async (after?: string) => {
    const page = await catalog<Page<Subscription>>(client.current!.memory, 'list-subscriptions', after ? { after } : {}); check();
    setSubscriptions(old => ({ ...page, items: after ? merge(old.items, page.items) : page.items })); return page;
  };
  const loadRecords = async (id: string, after?: string) => {
    const page = await readRecordValue<Page<RecordSummary>>('list-records', { subscriptionId: id, ...(after ? { after } : {}) }); check();
    setRecords(old => ({ ...page, items: after ? merge(old.items, page.items) : page.items }));
    // Existing summaries omit bodies. Hydrate only active recollections in this
    // bounded page, sequentially; reading never starts inference.
    for (const item of page.items) {
      if (item.kind === 'transcript-observation' || item.status !== 'active') continue;
      try {
        const value = await readRecordValue<MemoryRecord>('get-record', { recordId: item.id }); check();
        setBodies(old => ({ ...old, [value.id]: value })); fail(`Memory ${item.id}`);
      } catch (failure) { check(); fail(`Memory ${item.id}`, failure); }
    }
  };
  const loadSubscription = async (id: string) => {
    const domain = client.current!.memory;
    const value = await request<Subscription>(domain, 'get-subscription', { subscriptionId: id });
    const psy = await request<{ state: Affect | null }>(domain, 'get-affect', { subscriptionId: id });
    const cycles = await request<{ state: CycleState | null }>(domain, 'get-cycles', { subscriptionId: id });
    const automatic = await request<AutomaticStatus>(domain, 'automatic-status', { subscriptionId: id });
    const extracted = await request<{ state: ExtractionState }>(domain, 'get-extraction', { subscriptionId: id }); check();
    setSelected(value); setAffect(psy.state); setCycle(cycles.state); setAutomaticRuntime(automatic); setExtraction(extracted.state); return value;
  };
  const loadRecord = async (id: string) => {
    const value = await readRecordValue<MemoryRecord>('get-record', { recordId: id }); check();
    setBodies(old => ({ ...old, [id]: value })); setRecord(value); setDraft(value.body); setEditMode(false); setAuthoring(false);
    const applied = value.kind === 'extracted' ? null : await request<{ appraisal: Appraisal | null }>(client.current!.memory, 'get-appraisal', { recordId: id }); check();
    setAppraisal(applied?.appraisal ?? null); return value;
  };
  const clearPair = () => {
    setSelected(null); setRecords(empty()); setBodies({}); setRecord(null); setDraft(''); setAuthoring(false); setEditMode(false);
    setAffect(null); setCycle(null); setExtraction(null); setAutomaticRuntime(null); setAppraisal(null); setErrors({}); setNotice('');
  };
  const openPair = async (chat: string, observer: string) => {
    clearPair(); setConversationId(chat); setObserverId(observer);
    if (!chat || !observer) return;
    let after: string | undefined, found: Subscription | undefined;
    // The first catalog page alone cannot establish absence or uniqueness.
    do {
      const page = await loadSubscriptions(after);
      for (const sub of page.items) if (sub.conversationId === chat && sub.observerId === observer) {
        if (found) throw new Error('More than one memory exists for this character and conversation.');
        found = sub;
      }
      after = page.next ?? undefined;
    } while (after);
    if (found) { await loadSubscription(found.id); await loadRecords(found.id); }
  };
  const refresh = async () => {
    await loadCards(); await loadChats(); await loadSubscriptions();
    if (selected) { await loadSubscription(selected.id); await loadRecords(selected.id); }
    else await openPair(conversationId, observerId);
  };
  useEffect(() => {
    mounted.current = true;
    connectPanel().then(async bridge => {
      if (!mounted.current) { bridge.dispose(); return; }
      client.current = { bridge, memory: new DomainClient(bridge, 'rintawa/memory/v1'), cards: new DomainClient(bridge, 'rintawa/character/v1'), chats: new DomainClient(bridge, 'rintawa/chat/v1') };
      await run(async () => {
        setReady(true); await loadCards(); const chatPage = await loadChats(); const subs = await loadSubscriptions();
        const initial = bridge.initialState;
        const chat = typeof initial.conversationId === 'string' ? initial.conversationId : subs.items[0]?.conversationId ?? chatPage.items[0]?.id ?? '';
        const observer = typeof initial.observerId === 'string' ? initial.observerId : subs.items.find(s => s.conversationId === chat)?.observerId ?? chatPage.items.find(c => c.id === chat)?.conversation.characterId ?? '';
        await openPair(chat, observer);
      });
    }).catch(error => fail('Connection', error));
    return () => { mounted.current = false; abort.current?.abort(); if (!running.current) dispose(); };
  }, []);
  useEffect(() => {
    if (!ready || !selected) return;
    const id = selected.id, controller = new AbortController(); let active = true, inFlight = false, lastReport = '';
    const poll = async () => {
      if (inFlight || running.current || editing.current || document.visibilityState !== 'visible' || !client.current) return;
      inFlight = true;
      try {
        const value = await client.current.memory.request<AutomaticStatus>('automatic-status', { subscriptionId: id }, controller.signal);
        if (!active || running.current || editing.current) return;
        setAutomaticRuntime(value); fail('Update status'); const key = JSON.stringify(value.report);
        if (key !== lastReport && value.report?.result?.status === 'committed') await run(async () => { await loadSubscription(id); await loadRecords(id); });
        lastReport = key;
      } catch (error) { if (active) fail('Update status', error); } finally { inFlight = false; }
    };
    void poll(); const timer = setInterval(() => void poll(), 2000);
    return () => { active = false; clearInterval(timer); controller.abort(); };
  }, [ready, selected?.id]);
  const mutate = async (method: string, args: Record<string, unknown>, capture: Unknown, confirmed: () => Promise<void>): Promise<boolean> => {
    const domain = client.current!.memory;
    let operation: Operation | undefined; let outcomeKnown = false; setNotice('Saving…');
    try {
      check(); operation = await domain.begin(method, args); check();
      const result = await domain.wait(operation, abort.current?.signal); check();
      if (result.status === 'unknown') { setUnknown(capture); setNotice('The result could not be confirmed. Check saved state before continuing.'); return false; }
      outcomeKnown = true;
      if (result.status === 'failed') { setNotice(''); throw new Error(result.reason); }
      setNotice('Saved.'); await confirmed(); return true;
    } catch (failure) {
      const notStarted = failure instanceof DomainError || failure instanceof BridgeError ? failure.outcome === 'not-started' : false;
      if (mounted.current && !outcomeKnown && (operation || !notStarted)) { setUnknown(capture); setNotice('The result could not be confirmed. Check saved state before continuing.'); }
      else if (!outcomeKnown && mounted.current) setNotice('');
      throw failure;
    } finally { if (operation) await domain.release(operation).catch(() => {}); }
  };
  const reconcile = async (capture: Unknown) => {
    try {
      if (capture.kind === 'subscription') { await loadSubscription(capture.id); await loadRecords(capture.id); await loadSubscriptions(); }
      else {
        const item = await readRecordValue<MemoryRecord>('get-record', { recordId: capture.id }); check();
        setBodies(old => ({ ...old, [item.id]: item })); setRecord(item); await loadRecords(item.subscriptionId);
      }
      setUnknown({ ...capture, reconciled: true }); setNotice('Saved state checked. Review it before making another change; your draft has been kept.');
    } catch (failure) {
      if (['not-found', 'item-not-found'].includes(code(failure)) || ['not-found', 'item-not-found'].includes(message(failure))) {
        setUnknown({ ...capture, reconciled: true }); setNotice('The item is absent. This alone does not confirm that the change failed.');
      } else throw failure;
    }
  };
  const captureSub = (s: Subscription): Unknown => ({ id: s.id, kind: 'subscription', expectedRevision: s.revision, checkpoint: s.lastEntryId });
  const startSubMutation = (method: string, extra = {}) => {
    if (!selected) return; const captured = selected;
    void run(async () => { await mutate(method, { subscriptionId: captured.id, expectedRevision: captured.revision, ...extra }, captureSub(captured), async () => { await loadSubscription(captured.id); await loadRecords(captured.id); }); });
  };
  const affectAction = (method: string, extra: Record<string, unknown>) => {
    if (!selected) return; const captured = selected;
    void run(async () => { await mutate(method, { subscriptionId: captured.id, ...extra }, captureSub(captured), async () => { await loadSubscription(captured.id); if (record) await loadRecord(record.id); }); });
  };
  const savePolicy = (enabled: boolean, targetMessages: number) => {
    if (!selected) return; let captured = selected;
    void run(async () => {
      if (targetMessages !== automaticRuntime?.targetMessages) {
        if (!await mutate('begin-set-automatic-policy', { subscriptionId: captured.id, expectedRevision: captured.revision, targetMessages }, captureSub(captured), async () => { captured = await loadSubscription(captured.id); })) return;
      }
      if (enabled !== !!captured.automaticEnabled) await mutate('begin-set-automatic', { subscriptionId: captured.id, expectedRevision: captured.revision, enabled }, captureSub(captured), async () => { await loadSubscription(captured.id); });
    });
  };
  const createPair = () => {
    const bound = { observerId, conversationId };
    void run(async () => {
      const [id] = await client.current!.memory.allocate(1);
      if (!await mutate('begin-create-subscription', { subscriptionId: id, ...bound }, { id, kind: 'subscription' }, async () => { await loadSubscription(id); })) return;
      const created = await request<Subscription>(client.current!.memory, 'get-subscription', { subscriptionId: id });
      await mutate('begin-set-automatic-policy', { subscriptionId: id, expectedRevision: created.revision, targetMessages: 3 }, captureSub(created), async () => { await loadSubscription(id); await loadRecords(id); await loadSubscriptions(); });
    });
  };
  const updateRecord = (status: RecordSummary['status'], body: string) => {
    if (!record) return; const captured = record;
    void run(async () => { await mutate(captured.kind === 'extracted' ? 'begin-update-recollection' : 'begin-update-record', { recordId: captured.id, expectedRevision: captured.revision, body, status }, { id: captured.id, kind: 'record', expectedRevision: captured.revision }, async () => {
      await loadRecords(captured.subscriptionId);
      if (status === 'suppressed') { setRecord(null); setEditMode(false); setDraft(''); } else await loadRecord(captured.id);
    }); });
  };
  const locked = !ready || busy || !!unknown, pairLocked = locked || authoring || editMode;
  const name = (id: string) => cards.items.find(item => item.id === id)?.card.name ?? 'Character';
  const title = (id: string) => chats.items.find(item => item.id === id)?.conversation.title ?? 'Conversation';
  const characterName = name(observerId);
  const visibleRecords = records.items.filter(item => item.status === 'active' && item.kind !== 'transcript-observation');
  const state = cycle?.enabled ? cycle : affect?.enabled ? affect : null;
  const report = automaticRuntime?.report;
  const mood = state ? `${state.stress >= 60 ? 'Tense' : state.activation >= 60 ? 'Energised' : 'Calm'}${state.valence < -25 ? ', low mood' : state.valence > 25 ? ', positive mood' : ', neutral mood'}` : 'No mood analysis enabled';
  const recordLimit = record?.kind === 'extracted' ? 4096 : maxBody;
  const editor = <div className="memory-editor"><label className="sr-only" htmlFor="memory-body">Memory text</label><textarea id="memory-body" autoFocus value={draft} disabled={locked} onChange={e => setDraft(e.target.value)}/><small>{byteLength(draft)} / {recordLimit} UTF-8 bytes</small>
    {record && errors.action && <><Button disabled={locked} onClick={() => { const id = record.id; void run(async () => { const saved = await readRecordValue<MemoryRecord>('get-record', { recordId: id }); check(); setRecord(saved); setBodies(old => ({ ...old, [id]: saved })); setNotice('Saved version reloaded; your draft is unchanged. Review before saving again.'); }); }}>Check saved version, keep draft</Button><details><summary>Last loaded saved text</summary><p className="memory-body">{record.body}</p></details></>}
    <div className="actions"><Button className="primary" disabled={locked || !draft.trim() || byteLength(draft) > recordLimit || !!record && draft === record.body} onClick={() => {
      if (record) { updateRecord(record.status, draft); return; }
      if (!selected) return; const binding = selected.id, body = draft;
      void run(async () => { const [id] = await client.current!.memory.allocate(1); await mutate('begin-create-authored', { subscriptionId: binding, recordId: id, body }, { id, kind: 'record' }, async () => { await loadRecords(binding); await loadRecord(id); }); });
    }}>Save memory</Button><Button disabled={busy || !!unknown} onClick={() => { setDraft(record?.body ?? ''); setEditMode(false); setAuthoring(false); }}>Cancel</Button></div>
  </div>;
  return <main className="memory">
    <header><h1>Psychology</h1><small>Memory and mood in this story</small></header>
    <nav className="memory-path" aria-label="Memory context">
      <label><span className="sr-only">Conversation</span><select aria-label="Conversation" disabled={pairLocked} value={conversationId} onChange={e => {
        const chat = e.target.value;
        const observer = subscriptions.items.find(s => s.conversationId === chat)?.observerId ?? chats.items.find(c => c.id === chat)?.conversation.characterId ?? '';
        void run(() => openPair(chat, observer));
      }}><option value="">Choose conversation</option>{conversationId && !chats.items.some(c => c.id === conversationId) && <option value={conversationId}>Current conversation</option>}{chats.items.map(item => <option key={item.id} value={item.id}>{item.conversation.title}</option>)}</select></label>
      <span aria-hidden="true">/</span>
      <label><span className="sr-only">Character</span><select aria-label="Character" disabled={pairLocked || !conversationId} value={observerId} onChange={e => void run(() => openPair(conversationId, e.target.value))}><option value="">Choose character</option>{observerId && !cards.items.some(c => c.id === observerId) && <option value={observerId}>Current character</option>}{cards.items.map(item => <option key={item.id} value={item.id}>{item.card.name}</option>)}</select></label>
      {(cards.next || chats.next) && <details><summary>More choices</summary><div className="actions">{chats.next && <Button disabled={pairLocked} onClick={() => void run(async () => { await loadChats(chats.next!); })}>Load more conversations</Button>}{cards.next && <Button disabled={pairLocked} onClick={() => void run(async () => { await loadCards(cards.next!); })}>Load more characters</Button>}</div></details>}
    </nav>
    {!ready && <Status>Connecting…</Status>}{notice && <Status>{notice}</Status>}
    {Object.entries(errors).filter(([, error]) => error).map(([key, error]) => <Status key={key} error>{key.startsWith('Memory ') ? 'Could not load a memory' : key}: {error}</Status>)}
    {Object.values(errors).some(Boolean) && <Button disabled={locked || editing.current} onClick={() => void run(refresh)}>Retry loading</Button>}
    {unknown && <section className="memory-unknown"><h2>Check an unconfirmed change</h2><p>Do not repeat the change until you have checked the saved state.</p><div className="actions"><Button disabled={busy} onClick={() => void run(() => reconcile(unknown))}>Check saved state</Button><Button disabled={busy || !unknown.reconciled} onClick={() => { setUnknown(null); setNotice('Saved state reviewed. Your next change will be a new action.'); }}>I have reviewed it</Button></div><details><summary>Operation details</summary><small>{JSON.stringify(unknown)}</small></details></section>}
    {selected ? <>
      <div className="memory-heading"><div><h2>{characterName} in {title(conversationId)}</h2><small role="status">{busy ? 'Updating…' : !selected.enabled ? 'Memory paused' : report?.status === 'pending' ? 'Updating after a reply…' : report?.result?.status === 'waiting' ? `${report.result.available ?? 0} new messages · waiting for ${report.result.target}` : report?.result?.status === 'failed' ? 'An update needs attention' : 'Ready'}</small></div><Button className="primary" disabled={locked || !selected.enabled || editing.current} onClick={() => startSubMutation('begin-process')}>Update now</Button></div>
      <div className="memory-overview">
        <section className="memory-recollections"><h2>What {characterName} remembers<Button aria-label="Add memory" title="Add memory" disabled={locked || editMode || authoring} onClick={() => { setRecord(null); setDraft(''); setAuthoring(true); }}>+</Button></h2><small>Personal recollections, which may be mistaken. Opening them does not call AI.</small>
          {authoring && editor}
          <div className="memory-list">{visibleRecords.length === 0 && <p className="memory-empty">No memories on this page. Add one with +, or enable memory extraction in Advanced settings.</p>}{visibleRecords.map(item => {
            const value = bodies[item.id], expanded = record?.id === item.id;
            return <article className="memory-entry" key={item.id}><button className="memory-preview" aria-expanded={expanded} disabled={locked || authoring || editMode} onClick={() => { if (expanded) setRecord(null); else void run(async () => { await loadRecord(item.id); }); }}><span aria-hidden="true">{expanded ? '▾' : '▸'}</span><span>{value ? value.body : 'Memory text unavailable — open to retry'}</span></button>
              {expanded && <div className="memory-expanded">{editMode ? editor : <><p className="memory-body">{record.body}</p><small>{item.kind === 'authored' ? 'Written by you' : item.authorEdited ? 'Extracted from this conversation · edited by you' : 'Extracted from this conversation'}</small><div className="actions memory-record-actions"><Button aria-label="Edit memory" title="Edit memory" disabled={locked} onClick={() => { setDraft(record.body); setEditMode(true); }}>✎</Button><Button aria-label="Delete memory" title="Delete memory" disabled={locked} onClick={() => updateRecord('suppressed', record.body)}><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" aria-hidden="true"><path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7"/></svg></Button></div></>}</div>}
            </article>;
          })}</div>
          {records.next && <Button disabled={locked || editMode || authoring} onClick={() => void run(() => loadRecords(selected.id, records.next!))}>Load more memories</Button>}
        </section>
        <section className="memory-mood"><h2>How {characterName} feels</h2><p className="mood-label">{mood}</p>{state && <div className="mood-bars">{[{ label: 'Mood', value: state.valence, min: -100 }, { label: 'Energy', value: state.activation, min: 0 }, { label: 'Stress', value: state.stress, min: 0 }].map(axis => <label key={axis.label}><span>{axis.label}</span><meter min={axis.min} max={100} value={axis.value} aria-label={axis.label}/><small>{axis.value}</small></label>)}</div>}<small>Fictional emotional state. Configure analysis or author adjustments below.</small></section>
      </div>
      <section><label className="memory-toggle"><span><strong>Use in {characterName}'s replies</strong><small>Send eligible memories and mood through Narrator to the configured AI when generating a reply. Stored memories stay here when off.</small></span><input type="checkbox" role="switch" aria-label="Use memories and mood in replies" checked={!!selected.narrationEnabled} disabled={locked || editMode || authoring} onChange={() => startSubMutation('begin-set-narration', { enabled: !selected.narrationEnabled })}/></label></section>
      <AutomaticControls key={selected.id} enabled={!!selected.automaticEnabled} runtime={automaticRuntime} disabled={locked || editMode || authoring} onSave={savePolicy} usesAI={!!extraction?.settings.enabled || !!cycle?.enabled}/>
      <details className="memory-advanced"><summary>Advanced settings and sources</summary><div className="memory-advanced-content">
        <section><h2>Memory in this conversation</h2><Button disabled={pairLocked} onClick={() => startSubMutation('begin-set-enabled', { enabled: !selected.enabled })}>{selected.enabled ? 'Pause memory' : 'Resume memory'}</Button><small>Pausing keeps saved memories and stops processing and narration context for this pair.</small></section>
        <ExtractionControls key={selected.id} state={extraction} disabled={pairLocked} onConfigure={settings => startSubMutation('begin-configure-extraction', { settings })}/>
        <CycleControls key={selected.id} state={cycle} disabled={pairLocked} onConfigure={(enabled, settings) => affectAction('begin-configure-cycles', { expectedSubscriptionRevision: selected.revision, expectedStateRevision: cycle?.revision ?? null, enabled, settings })}/>
        <details><summary>Authored mood and simulation time</summary><AffectControls state={affect} disabled={pairLocked || !!cycle?.enabled} onToggle={() => affectAction('begin-set-affect', { expectedSubscriptionRevision: selected.revision, expectedStateRevision: affect?.revision ?? null, enabled: !affect?.enabled })} onAdvance={seconds => affectAction('begin-advance-affect', { expectedStateRevision: affect!.revision, elapsedSeconds: seconds })}/></details>
        <details><summary>Source records and removed memories</summary><section><small>Deleting a memory removes it from active recall. Source evidence and the removed record remain available here.</small>{records.items.map(item => <Button key={item.id} disabled={locked || authoring || editMode} onClick={() => void run(async () => { await loadRecord(item.id); })}>{item.kind} · {item.status} · {item.id.slice(0, 8)}</Button>)}{records.next && <Button disabled={locked} onClick={() => void run(() => loadRecords(selected.id, records.next!))}>Load more source records</Button>}
          {record && <div className="memory-source"><h3>Selected record</h3><p className="memory-body">{record.body}</p><small>{record.id} · revision {record.revision} · {record.status}</small>{record.evidence && <pre>{JSON.stringify(record.evidence, null, 2)}</pre>}{record.provenance && <pre>{JSON.stringify(record.provenance, null, 2)}</pre>}{record.status === 'suppressed' && <Button disabled={locked} onClick={() => updateRecord('active', record.body)}>Restore memory</Button>}{record.kind !== 'extracted' && <AppraisalControls key={record.id} state={affect} appraisal={appraisal} recordRevision={record.revision} disabled={locked || !!cycle?.enabled || record.status !== 'active'} onAppraise={(seconds, deltas: Deltas) => affectAction('begin-appraise', { recordId: record.id, expectedRecordRevision: record.revision, expectedStateRevision: affect!.revision, elapsedSeconds: seconds, deltas })}/>}</div>}
        </section></details>
        <details><summary>Processing details</summary><section><small>Subscription {selected.id} · revision {selected.revision}<br/>Checkpoint {selected.lastEntryId ?? 'Not processed'}</small><pre>{JSON.stringify(automaticRuntime?.report ?? null, null, 2)}</pre><small>Recent activity resets after restart. A missing result does not prove success. Update now processes one bounded batch; repeat explicitly for remaining history.</small></section></details>
      </div></details>
    </> : <section className="memory-empty"><h2>{conversationId && observerId ? `Give ${characterName} a memory in this story` : 'Choose a conversation and character'}</h2><p>Each character has their own memories and mood in each conversation.</p>{conversationId && observerId && <Button className="primary" disabled={locked || Object.values(errors).some(Boolean)} onClick={createPair}>Set up memory</Button>}</section>}
  </main>;
}
createRoot(document.getElementById('root')!).render(<App />);
