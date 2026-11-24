import { useEffect, useRef, useState } from 'react';
import { BridgeError, type Client, type ResourceRef, type ViewState } from '@rintawa/bridge';
import { DomainClient, DomainError, observeResource, type Observation, type Operation } from '@rintawa/domain';
import { Button, Icon, Status } from '@rintawa/ui';

type Card = { id: string; card: { name: string }; greeting: { text: string | null; error: string | null } };
type Conversation = { id: string; conversation: { title: string; characterId: string | null } };
type Page<T> = { items: T[]; next: string | null };
const uuid = (value: unknown): value is string => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);
const text = (error: unknown) => error instanceof Error ? error.message : 'Request unavailable';
const notStarted = (error: unknown) => (error instanceof DomainError || error instanceof BridgeError) && error.outcome === 'not-started';

/** The selected Character UI coordinates two owner transactions, never replays them. */
export function Story({ client }: { client: Client }) {
  const character = useRef(new DomainClient(client, 'rintawa/character/v1'));
  const chat = useRef(new DomainClient(client, 'rintawa/chat/v1'));
  const cardObservation = useRef<Observation | null>(null); const chatObservation = useRef<Observation | null>(null);
  const cardPages = useRef(1); const chatPages = useRef(1);
  const live = useRef(true); const abort = useRef(new AbortController());
  const active = useRef<{ domain: DomainClient; operation: Operation } | null>(null);
  const uploaded = useRef<ResourceRef | null>(null);
  const saved = useRef<ViewState>({});
  const [ready, setReady] = useState(false); const [busy, setBusy] = useState(false);
  const [cardError, setCardError] = useState(''); const [chatError, setChatError] = useState('');
  const [error, setError] = useState(''); const [notice, setNotice] = useState('');
  const [cards, setCards] = useState<Card[]>([]); const [cardCursor, setCardCursor] = useState<string | null>(null);
  const [conversations, setConversations] = useState<Conversation[]>([]); const [chatCursor, setChatCursor] = useState<string | null>(null);
  const [recovery, setRecovery] = useState<Card | null>(null);
  const [uncertain, setUncertain] = useState(false);
  const [committedConversationId, setCommittedConversationId] = useState<string | null>(null);
  const save = async (state: ViewState) => { await client.setState(state); saved.current = state; };
  const open = async (conversationId: string) => {
    if (!uuid(conversationId)) throw new Error('Invalid conversation ID');
    // The selected Chat owner supplies the exact companion view identity.
    await chat.current.request('recent', { conversationId });
    setCommittedConversationId(conversationId); setUncertain(false);
    await save({ conversationId, phase: 'committed' });
    const target = await chat.current.request<{ instanceId: string; componentId: string }>('describe');
    await client.navigate(target.instanceId, target.componentId, { conversationId });
  };
  const refresh = async () => { await Promise.all([cardObservation.current?.refresh(), chatObservation.current?.refresh()]); };
  // Restoration can navigate. Only startup and explicit recovery call it; catalog hints are pure reads.
  const restore = async () => {
    const state = saved.current;
    if (uuid(state.conversationId)) {
      if (state.phase === 'committed') setCommittedConversationId(state.conversationId);
      try { await open(state.conversationId); return; }
      catch (failure) {
        if (saved.current.phase === 'committed' || !(failure instanceof DomainError) || failure.code !== 'not-found') throw failure;
      }
    }
    if (uuid(state.characterId)) {
      try {
        const card = await character.current.request<Card>('get', { itemId: state.characterId });
        if (live.current) setRecovery(card);
        // A known import COMMIT can be followed by an explicit continuation.
        setUncertain(state.phase === 'creating');
      } catch (failure) {
        if (!(failure instanceof DomainError) || failure.code !== 'not-found') throw failure;
        setUncertain(true); setNotice('Import outcome unknown. Read the World again before starting another import.');
      }
    }
  };
  useEffect(() => {
    live.current = true;
    cardObservation.current = observeResource<Page<Card>>({
      client, points: ['rintawa/character/changed/v1'],
      read: async signal => {
        const items: Card[] = []; let next: string | null = null;
        for (let page = 0; page < cardPages.current; page++) {
          const result: Page<Card> = await character.current.request<Page<Card>>('list', next ? { after: next } : {}, signal);
          items.push(...result.items); next = result.next;
          if (!next) break;
        }
        return { items, next };
      },
      apply: result => { setCards(result.items); setCardCursor(result.next); setCardError(''); },
      failed: failure => setCardError(`Characters may be stale: ${text(failure)}`),
    });
    chatObservation.current = observeResource<Page<Conversation>>({
      client, points: ['rintawa/chat/changed/v1'],
      read: async signal => {
        const items: Conversation[] = []; let next: string | null = null;
        for (let page = 0; page < chatPages.current; page++) {
          const result: Page<Conversation> = await chat.current.request<Page<Conversation>>('list', next ? { after: next } : {}, signal);
          items.push(...result.items); next = result.next;
          if (!next) break;
        }
        return { items, next };
      },
      apply: result => { setConversations(result.items); setChatCursor(result.next); setChatError(''); },
      failed: failure => setChatError(`Chats may be stale: ${text(failure)}`),
    });
    (async () => {
      saved.current = await client.getState();
      await Promise.allSettled([cardObservation.current?.refresh(), chatObservation.current?.refresh()]);
      if (live.current) await restore();
    })()
      .catch(failure => { if (live.current) setError(text(failure)); })
      .finally(() => { if (live.current) setReady(true); });
    return () => {
      live.current = false; cardObservation.current?.close(); chatObservation.current?.close(); abort.current.abort();
      if (active.current) void active.current.domain.release(active.current.operation).catch(() => {});
      if (uploaded.current) void character.current.request('discard-resource', { resource: uploaded.current }).catch(() => {});
    };
  }, [client]);
  const perform = async (work: () => Promise<void>) => {
    setBusy(true); setError(''); setNotice('');
    try { await work(); } catch (failure) { if (live.current) setError(text(failure)); }
    finally { if (live.current) setBusy(false); }
  };
  const wait = async (domain: DomainClient, operation: Operation) => {
    active.current = { domain, operation };
    if (!live.current) { void domain.release(operation).catch(() => {}); throw new Error('View closed'); }
    const result = await domain.wait(operation, abort.current.signal); active.current = null;
    return result;
  };
  const createChat = async (card: Card, knownId?: string, skipGreeting = false) => {
    if (committedConversationId) { await open(committedConversationId); return; }
    const [conversationId] = knownId ? [knownId] : await chat.current.allocate(1);
    const current = await character.current.request<Card>('get', { itemId: card.id });
    if (current.greeting.error && !skipGreeting) throw new Error('Character imported, but its greeting exceeds the message limit. No Chat was created.');
    let greeting: { messageId: string; entryId: string; text: string } | undefined;
    if (current.greeting.text && !skipGreeting) {
      const text = current.greeting.text.replace(/\{\{(char|user)\}\}/g, (_, key) => key === 'char' ? current.card.name : 'You');
      const [messageId, entryId] = await chat.current.allocate(2);
      greeting = { messageId, entryId, text };
    }
    await save({ characterId: card.id, conversationId, phase: 'creating' });
    let sent = false; let terminal = false; let committed = false;
    try {
      sent = true;
      const operation = await chat.current.begin('begin-create', { conversationId, characterId: card.id, title: card.card.name, ...(greeting ? { greeting } : {}) });
      const result = await wait(chat.current, operation);
      terminal = result.status !== 'unknown';
      if (result.status === 'failed') {
        await save({ characterId: card.id, phase: 'imported' });
        setRecovery(card); setUncertain(false); throw new Error(result.reason);
      }
      if (result.status === 'unknown') { setRecovery(card); setUncertain(true); setNotice('Chat outcome unknown. Read the World to check the known conversation ID.'); return; }
      committed = true; setCommittedConversationId(conversationId);
      await save({ characterId: card.id, conversationId, phase: 'committed' });
      void chatObservation.current?.refresh().catch(() => {});
      await open(conversationId);
    } catch (failure) {
      if (committed) {
        setRecovery(card); setUncertain(false); setNotice('Chat saved. Retry opening the same conversation.');
      } else if (sent && !terminal && !notStarted(failure)) { setRecovery(card); setUncertain(true); }
      else { await save({ characterId: card.id, phase: 'imported' }); setRecovery(card); setUncertain(false); }
      throw failure;
    }
  };
  const importCard = async (file: File) => {
    const extension = file.name.toLowerCase().split('.').pop();
    const limit = extension === 'json' ? 4 * 1024 * 1024 : extension === 'png' ? 16 * 1024 * 1024 : 0;
    if (!limit || !file.size || file.size > limit) throw new Error('Choose Tavern JSON up to 4 MiB or PNG up to 16 MiB.');
    let sent = false; let imported = false;
    try {
      const resource = await client.upload(file, file.name); uploaded.current = resource;
      if (!live.current) return;
      const [characterId, conversationId] = await character.current.allocate(2);
      await save({ characterId, conversationId, phase: 'importing' });
      if (!live.current) return;
      setNotice('Importing…'); sent = true;
      const operation = await character.current.begin('begin-import', { itemId: characterId, resource });
      const result = await wait(character.current, operation);
      if (result.status === 'failed') { await save({}); setUncertain(false); throw new Error(result.reason); }
      if (result.status === 'unknown') { setUncertain(true); setNotice('Import outcome unknown. Read the World before trying again.'); return; }
      imported = true;
      const card = await character.current.request<Card>('get', { itemId: characterId });
      void cardObservation.current?.refresh().catch(() => {});
      await save({ characterId, phase: 'imported' }); setRecovery(card); setNotice('Opening chat…');
      await createChat(card, conversationId);
    } catch (failure) {
      if (!imported && sent && !notStarted(failure) && saved.current.phase === 'importing') setUncertain(true);
      else if (!imported && notStarted(failure)) await save({});
      throw failure;
    } finally {
      if (uploaded.current) { void character.current.request('discard-resource', { resource: uploaded.current }).catch(() => {}); uploaded.current = null; }
    }
  };
  return <main className="story-welcome" aria-label="Start your story"><div className="story-welcome-content">
    <span className="story-symbol"><Icon name="user" size={34} /></span>
    <h1>{recovery ? recovery.card.name : conversations.length || committedConversationId ? 'Continue your story' : 'Import a character'}</h1>
    {!recovery && !conversations.length && !committedConversationId && <p>Tavern V2 / V3 · PNG or JSON</p>}
    {!ready && <Status>Connecting…</Status>}{busy && <Status>Working…</Status>}{error && <Status error>{error}</Status>}{notice && <Status>{notice}</Status>}
    {cardError && <><Status error>{cardError}</Status><Button disabled={busy} onClick={() => void cardObservation.current?.refresh().catch(() => {})}>Retry characters</Button></>}
    {chatError && <><Status error>{chatError}</Status><Button disabled={busy} onClick={() => void chatObservation.current?.refresh().catch(() => {})}>Retry chats</Button></>}
    {committedConversationId && <Button className="primary" disabled={busy || !ready} onClick={() => void perform(() => open(committedConversationId))}>Retry opening chat</Button>}
    {recovery && !committedConversationId && <Button className="primary" disabled={busy || !ready || uncertain} onClick={() => void perform(() => createChat(recovery, undefined, !!recovery.greeting.error))}>{recovery.greeting.error ? 'Start without greeting' : 'Continue to chat'}</Button>}
    {uncertain && <Button disabled={busy} onClick={() => void perform(async () => { await refresh(); if (live.current) await restore(); })}>Read World</Button>}
    {!!conversations.length && <div className="story-choices">{conversations.map(row => <Button key={row.id} disabled={busy} onClick={() => void perform(() => open(row.id))}><Icon name="chat" size={16} />{row.conversation.title}</Button>)}</div>}
    {!recovery && !committedConversationId && cards.filter(card => !conversations.some(row => row.conversation.characterId === card.id)).map(card => <Button key={card.id} disabled={busy || uncertain} onClick={() => void perform(() => createChat(card))}>Continue with {card.card.name}</Button>)}
    {!uncertain && !recovery && !committedConversationId && <><label htmlFor="story-character-file" className={`import-button${!ready || busy ? ' disabled' : ''}`} aria-disabled={!ready || busy}>Import character</label><input id="story-character-file" className="file-input" type="file" accept=".png,.json" disabled={!ready || busy} onChange={event => { const file = event.currentTarget.files?.[0]; event.currentTarget.value = ''; if (file) void perform(() => importCard(file)); }} /></>}
    {(cardCursor || chatCursor) && <Button disabled={busy || !!cardError || !!chatError} onClick={() => void perform(async () => {
      const reads: Promise<void>[] = [];
      if (chatCursor) { chatPages.current++; if (chatObservation.current) reads.push(chatObservation.current.refresh()); }
      if (cardCursor) { cardPages.current++; if (cardObservation.current) reads.push(cardObservation.current.refresh()); }
      await Promise.allSettled(reads);
    })}>More</Button>}
  </div></main>;
}
