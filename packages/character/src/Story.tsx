import { useEffect, useRef, useState } from 'react';
import { BridgeError, type Client, type ResourceRef, type ViewState } from '@rintawa/bridge';
import { DomainClient, DomainError, type Operation } from '@rintawa/domain';
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
  const live = useRef(true); const abort = useRef(new AbortController());
  const active = useRef<{ domain: DomainClient; operation: Operation } | null>(null);
  const uploaded = useRef<ResourceRef | null>(null);
  const saved = useRef<ViewState>({});
  const [ready, setReady] = useState(false); const [busy, setBusy] = useState(false);
  const [error, setError] = useState(''); const [notice, setNotice] = useState('');
  const [cards, setCards] = useState<Card[]>([]); const [cardCursor, setCardCursor] = useState<string | null>(null);
  const [conversations, setConversations] = useState<Conversation[]>([]); const [chatCursor, setChatCursor] = useState<string | null>(null);
  const [recovery, setRecovery] = useState<Card | null>(null);
  const [uncertain, setUncertain] = useState(false);
  const save = async (state: ViewState) => { await client.setState(state); saved.current = state; };
  const open = async (conversationId: string) => {
    if (!uuid(conversationId)) throw new Error('Invalid conversation ID');
    // The selected Chat owner supplies the exact companion view identity.
    await chat.current.request('recent', { conversationId });
    const target = await chat.current.request<{ instanceId: string; componentId: string }>('describe');
    await save({ conversationId });
    await client.navigate(target.instanceId, target.componentId, { conversationId });
  };
  const discover = async () => {
    const [characters, chats] = await Promise.all([
      character.current.request<Page<Card>>('list'), chat.current.request<Page<Conversation>>('list'),
    ]);
    if (!live.current) return;
    setCards(characters.items); setCardCursor(characters.next);
    setConversations(chats.items); setChatCursor(chats.next);
    const state = saved.current;
    if (uuid(state.conversationId)) {
      try { await open(state.conversationId); return; }
      catch (failure) { if (!(failure instanceof DomainError) || failure.code !== 'not-found') throw failure; }
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
    (async () => { saved.current = await client.getState(); await discover(); })()
      .catch(failure => { if (live.current) setError(text(failure)); })
      .finally(() => { if (live.current) setReady(true); });
    return () => {
      live.current = false; abort.current.abort();
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
    let sent = false; let terminal = false;
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
      await open(conversationId);
    } catch (failure) {
      if (sent && !terminal && !notStarted(failure)) { setRecovery(card); setUncertain(true); }
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
    <h1>{recovery ? recovery.card.name : conversations.length ? 'Continue your story' : 'Import a character'}</h1>
    {!recovery && !conversations.length && <p>Tavern V2 / V3 · PNG or JSON</p>}
    {!ready && <Status>Connecting…</Status>}{busy && <Status>Working…</Status>}{error && <Status error>{error}</Status>}{notice && <Status>{notice}</Status>}
    {recovery && <Button className="primary" disabled={busy || !ready || uncertain} onClick={() => void perform(() => createChat(recovery, undefined, !!recovery.greeting.error))}>{recovery.greeting.error ? 'Start without greeting' : 'Continue to chat'}</Button>}
    {uncertain && <Button disabled={busy} onClick={() => void perform(discover)}>Read World</Button>}
    {!!conversations.length && <div className="story-choices">{conversations.map(row => <Button key={row.id} disabled={busy} onClick={() => void perform(() => open(row.id))}><Icon name="chat" size={16} />{row.conversation.title}</Button>)}</div>}
    {!recovery && cards.filter(card => !conversations.some(row => row.conversation.characterId === card.id)).map(card => <Button key={card.id} disabled={busy || uncertain} onClick={() => void perform(() => createChat(card))}>Continue with {card.card.name}</Button>)}
    {!uncertain && !recovery && <><label htmlFor="story-character-file" className={`import-button${!ready || busy ? ' disabled' : ''}`} aria-disabled={!ready || busy}>Import character</label><input id="story-character-file" className="file-input" type="file" accept=".png,.json" disabled={!ready || busy} onChange={event => { const file = event.currentTarget.files?.[0]; event.currentTarget.value = ''; if (file) void perform(() => importCard(file)); }} /></>}
    {(cardCursor || chatCursor) && <Button disabled={busy} onClick={() => void perform(async () => {
      if (chatCursor) { const page = await chat.current.request<Page<Conversation>>('list', { after: chatCursor }); setConversations(previous => [...previous, ...page.items]); setChatCursor(page.next); }
      if (cardCursor) { const page = await character.current.request<Page<Card>>('list', { after: cardCursor }); setCards(previous => [...previous, ...page.items]); setCardCursor(page.next); }
    })}>More</Button>}
  </div></main>;
}
