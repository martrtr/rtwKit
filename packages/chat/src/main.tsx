import { useMessageActions } from './useMessageActions';
import { ComposerActions } from './ComposerActions';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, BridgeError, type Client, type AssetRef, type ViewState } from '@rintawa/bridge';
import { DomainClient, DomainError, type Operation, type OperationStatus, observeResource, type Observation } from '@rintawa/domain';
import { Button, Dialog, Icon, Input, SplitPane, Status } from '@rintawa/ui';
import '@rintawa/ui/style.css';
import './style.css';

type ReplyPreview = { conversationId: string; messageId: string; text: string; phase: 'streaming' | 'interrupted' | 'saved' | 'unknown' };
type Conversation = { revision: string; title: string; characterId: string | null; lastEntryId: string | null };
type ConversationRow = { id: string; conversation: Conversation };
type Entry = { entryId: string; messageId: string; message: { role: string; text: string; revision: string } };
type Recent = { conversation: Conversation; items: Entry[]; older: string | null; head: string | null };
type Character = { id: string; card: { name: string; portrait: AssetRef | null } };
type CharacterList = { items: Character[]; next: string | null };
type ConversationList = { items: ConversationRow[]; next: string | null };
const message = (error: unknown) => error instanceof Error ? error.message : 'Request unavailable';

function aiFailure(reason: string): string {
  if (reason.includes('stale-context')) return 'The conversation changed. Review the latest messages before retrying.';
  const messages: Record<string, string> = {
    'primary-provider-required': 'Choose a primary provider in AI settings.',
    'shared-connections-unavailable': 'Start Ai Provider on Home using the same installed package version.',
    'shared-connections-permission-required': 'Allow shared connections for this World’s AI backend, choose Allow access in AI settings.',
    'connection-unavailable': 'The primary provider was removed. Choose a primary provider in AI settings.',
    'credentials-required': 'The AI Provider requires a local API key. Use API key setup in AI settings.',
    'secret-read-permission-required': 'The AI Provider requires a local API key read permission. Choose Allow access in AI settings.',
    'credential-store-unavailable': 'Unlock your system credential store and retry explicitly.',
    'http-permission-required': 'Choose Allow access in AI settings to connect to the local server.',
    'http-timeout': 'Response timed out.',
    'character-context-too-large': 'The character prompt exceeds the context budget. Its content was not truncated.',
    'prompt-too-large': 'The character and current message exceed the context budget. Try a shorter message.',
    'answer-too-large': 'Response exceeded the size limit.',
    'incomplete-provider-stream': 'Response interrupted.',
  };
  return messages[reason] ?? reason;
}
function Chat() {
  const bridge = useRef<Client | null>(null);
  const domain = useRef<DomainClient | null>(null);
  const characters = useRef<DomainClient | null>(null);
  const narrator = useRef<DomainClient | null>(null);
  const aiOperation = useRef<Operation | null>(null);
  const aiAbort = useRef<AbortController | null>(null);
  const [aiAvailable, setAiAvailable] = useState(false);
  const [aiBusy, setAiBusy] = useState(false);
  const [replyPreview, setReplyPreview] = useState<ReplyPreview | null>(null);
  const followingReply = useRef(false);
  const transcript = useRef<HTMLDivElement | null>(null);
  const scrollAnchor = useRef<{ element: HTMLElement; offset: number } | null>(null);
  const active = useRef<Operation | null>(null);
  const abort = useRef<AbortController | null>(null);
  const mounted = useRef(true);
  const [ready, setReady] = useState(false);
  const [navigationOpen, setNavigationOpen] = useState(false);
  const [navigationRatio, setNavigationRatio] = useState(.25);
  const viewState = useRef<ViewState>({});
  const pendingState = useRef<ViewState | null>(null);
  const savingState = useRef(false);
  const navigationToggle = useRef<HTMLButtonElement>(null);
  // Coalesce layout/selection preferences, with one save in flight. A resize
  // must not overwrite a newer conversation selection (or vice versa).
  const saveState = (patch: ViewState) => {
    viewState.current = { ...viewState.current, ...patch };
    pendingState.current = viewState.current;
    if (savingState.current || !bridge.current) return;
    savingState.current = true;
    void (async () => {
      try {
        while (pendingState.current && mounted.current) {
          const next = pendingState.current; pendingState.current = null;
          await bridge.current!.setState(next);
        }
      } catch (failure) { pendingState.current = null; if (mounted.current) setError(message(failure)); }
      finally { savingState.current = false; }
    })();
  };
  const toggleNavigation = (open: boolean) => { setNavigationOpen(open); saveState({ navigationOpen: String(open) }); };
  const resizeNavigation = (ratio: number) => { setNavigationRatio(ratio); saveState({ navigationRatio: String(ratio) }); };
  const [createOpen, setCreateOpen] = useState(false);
  const [search, setSearch] = useState('');
  const composer = useRef<HTMLTextAreaElement | null>(null);
  const [working, setBusy] = useState(false);
  const busy = working || aiBusy;
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [unknown, setUnknown] = useState<string[] | null>(null);
  const [conversations, setConversations] = useState<ConversationRow[]>([]);
  const [listCursor, setListCursor] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [pageConversation, setPageConversation] = useState<Conversation | null>(null);
  const [entries, setEntries] = useState<Entry[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [cards, setCards] = useState<Character[]>([]);
  const [cardCursor, setCardCursor] = useState<string | null>(null);
  const [characterId, setCharacterId] = useState('');
  const [linked, setLinked] = useState<Character | null>(null);
  const [portrait, setPortrait] = useState<string | null>(null);
  const [title, setTitle] = useState('');
  const [text, setText] = useState('');
  const messageActions = useMessageActions(bridge.current, selected);
  const listWatch = useRef<Observation | null>(null), cardsWatch = useRef<Observation | null>(null), threadWatch = useRef<Observation | null>(null);
  const listPages = useRef(1), cardPages = useRef(1), threadPages = useRef(1);
  const selectedRef = useRef(selected); selectedRef.current = selected;
  const [readErrors, setReadErrors] = useState<Record<string, string>>({});
  const readFailure = (key: string, failure?: unknown) => { if (mounted.current) setReadErrors(old => ({ ...old, [key]: failure ? message(failure) : '' })); };
  const listFingerprint = useRef(''), cardFingerprint = useRef('');

  const loadList = async (after?: string) => { if (after) listPages.current++; await listWatch.current?.refresh(); };
  const loadCards = async (after?: string) => { if (after) cardPages.current++; await cardsWatch.current?.refresh(); };
  const loadLatest = async (id: string) => { if (id === selectedRef.current) await threadWatch.current?.refresh(); };
  const loadOlder = async (id: string, _before: string) => { if (id === selectedRef.current) { threadPages.current++; await threadWatch.current?.refresh(); } };
  useEffect(() => {
    mounted.current = true;
    connectPanel().then(async client => {
      if (!mounted.current) { client.dispose(); return; }
      bridge.current = client;
      domain.current = new DomainClient(client, 'rintawa/chat/v1');
      characters.current = new DomainClient(client, 'rintawa/character/v1');
      narrator.current = new DomainClient(client, 'rintawa/narrator/v1');
      listWatch.current = observeResource({ client, points: ['rintawa/chat/changed/v1'],
        read: async signal => {
          const items: ConversationRow[] = []; let next: string | null = null;
          for (let page = 0; page < listPages.current; page++) {
            const result: ConversationList = await domain.current!.request('list', next ? { after: next } : {}, signal);
            items.push(...result.items); next = result.next; if (!next) break;
          }
          return { items, next };
        }, apply: value => { readFailure('conversations'); const key = JSON.stringify(value); if (key !== listFingerprint.current) { listFingerprint.current = key; setConversations(value.items); setListCursor(value.next); } }, failed: error => readFailure('conversations', error),
      });
      cardsWatch.current = observeResource({ client, points: ['rintawa/character/changed/v1'],
        read: async signal => {
          const items: Character[] = []; let next: string | null = null;
          for (let page = 0; page < cardPages.current; page++) {
            const result: CharacterList = await characters.current!.request('list', next ? { after: next } : {}, signal);
            items.push(...result.items); next = result.next; if (!next) break;
          }
          return { items, next };
        }, apply: value => { readFailure('characters'); const key = JSON.stringify(value); if (key !== cardFingerprint.current) { cardFingerprint.current = key; setCards(value.items); setCardCursor(value.next); } }, failed: error => readFailure('characters', error),
      });
      void (async () => {
        for (let attempt = 0; attempt < 5 && mounted.current; attempt++) {
          try { const value = await narrator.current!.request<{ available: boolean }>('describe'); if (mounted.current) setAiAvailable(value.available); return; }
          catch (failure) { if (!(failure instanceof DomainError || failure instanceof BridgeError) || !['unavailable', 'limit-exceeded'].includes(failure.code)) return; }
          await new Promise(resolve => setTimeout(resolve, 80));
        }
      })();
      // A mounted panel can precede the backend's query declaration. Retry only
      // these startup reads; admission of writes and generation stays single-shot.
      const bootstrapRead = async (action: () => Promise<void>) => {
        for (let attempt = 0; ; attempt++) {
          if (!mounted.current) throw new DOMException('Panel closed', 'AbortError');
          try { await action(); return; }
          catch (failure) {
            if (attempt >= 19 || !(failure instanceof DomainError || failure instanceof BridgeError)
              || !['unavailable', 'limit-exceeded'].includes(failure.code)) throw failure;
          }
          await new Promise(resolve => setTimeout(resolve, 100));
        }
      };
      void bootstrapRead(() => loadCards()).catch(() => {});
      try {
        await bootstrapRead(() => loadList());
        const state = await client.getState();
        viewState.current = { ...state, ...client.initialState };
        const savedRatio = viewState.current.navigationRatio === undefined ? NaN : Number(viewState.current.navigationRatio);
        if (Number.isFinite(savedRatio) && savedRatio >= 0 && savedRatio <= 1) setNavigationRatio(savedRatio);
        setNavigationOpen(viewState.current.navigationOpen === 'true');
        const conversationId = viewState.current.conversationId;
        if (conversationId && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(conversationId)) {
          if (mounted.current) { setSelected(conversationId); saveState({ conversationId }); }
        }
      } catch (failure) { if (mounted.current) setError(message(failure)); }
      finally { if (mounted.current) setReady(true); }
    }).catch(failure => { if (mounted.current) setError(message(failure)); });
    return () => {
      mounted.current = false;
      abort.current?.abort();
      aiAbort.current?.abort();
      if (aiOperation.current && narrator.current) void narrator.current.release(aiOperation.current).catch(() => {});
      if (active.current && domain.current) void domain.current.release(active.current).catch(() => {});
      listWatch.current?.close(); cardsWatch.current?.close(); threadWatch.current?.close();
      bridge.current?.dispose();
    };
  }, []);

  useEffect(() => {
    if (!ready || !selected || !bridge.current || !domain.current) return;
    const id = selected; threadPages.current = 1; let fingerprint = '';
    const watch = observeResource({ client: bridge.current, points: ['rintawa/chat/changed/v1'], resource: id,
      read: async signal => {
        let before: string | null = null, latest: Recent | null = null; const items: Entry[] = [];
        for (let page = 0; page < threadPages.current; page++) {
          const result: Recent = await domain.current!.request('recent', { conversationId: id, ...(before ? { before } : {}) }, signal);
          if (latest && JSON.stringify(latest.conversation) !== JSON.stringify(result.conversation)) throw new Error('Conversation changed while reading; retrying.');
          latest ??= result; items.unshift(...result.items); before = result.older; if (!before) break;
        }
        return { ...latest!, items, older: before };
      }, apply: value => {
        readFailure('transcript'); const key = JSON.stringify(value); if (key === fingerprint) return; fingerprint = key;
        const view = transcript.current;
        const nearEnd = !view || view.scrollHeight - view.scrollTop - view.clientHeight < 64;
        const top = view?.getBoundingClientRect().top ?? 0;
        const element = !nearEnd && view && [...view.querySelectorAll<HTMLElement>('article.message')].find(row => row.getBoundingClientRect().bottom > top);
        scrollAnchor.current = element ? { element, offset: element.getBoundingClientRect().top - top } : null;
        setPageConversation(value.conversation); setEntries(value.items); setCursor(value.older);
      }, failed: error => readFailure('transcript', error),
    });
    threadWatch.current = watch;
    return () => { watch.close(); if (threadWatch.current === watch) threadWatch.current = null; };
  }, [ready, selected]);

  useEffect(() => {
    aiAbort.current?.abort();
    setReplyPreview(null);
  }, [selected]);
  useEffect(() => {
    if (replyPreview && entries.some(entry => entry.messageId === replyPreview.messageId)) setReplyPreview(null);
  }, [entries, replyPreview]);
  useLayoutEffect(() => {
    if (replyPreview?.phase === 'streaming' && followingReply.current && transcript.current) transcript.current.scrollTo({ top: transcript.current.scrollHeight, behavior: 'instant' });
  }, [replyPreview?.text, replyPreview?.messageId]);

  useEffect(() => { if (composer.current) { composer.current.style.height = 'auto'; composer.current.style.height = Math.min(composer.current.scrollHeight, 160) + 'px'; } }, [text]);

  const selectedConversation = pageConversation ?? conversations.find(row => row.id === selected)?.conversation;
  useEffect(() => {
    let cancelled = false;
    setLinked(null);
    if (selectedConversation?.characterId && characters.current) void characters.current.request<Character>('get', { itemId: selectedConversation.characterId }).then(card => { if (!cancelled) setLinked(card); }).catch(() => {});
    return () => { cancelled = true; };
  }, [selectedConversation?.characterId]);
  useEffect(() => {
    let cancelled = false; let revoke: (() => void) | undefined; setPortrait(null);
    if (linked?.card.portrait && bridge.current) void bridge.current.image(linked.card.portrait).then(image => {
      if (cancelled) image.revoke(); else { revoke = image.revoke; setPortrait(image.url); }
    }).catch(() => {});
    return () => { cancelled = true; revoke?.(); };
  }, [linked]);
  useLayoutEffect(() => {
    const view = transcript.current;
    const anchor = scrollAnchor.current;
    scrollAnchor.current = null;
    if (view) {
      const top = anchor?.element.isConnected
        ? view.scrollTop + anchor.element.getBoundingClientRect().top - view.getBoundingClientRect().top - anchor.offset
        : view.scrollHeight;
      view.scrollTo({ top, behavior: 'instant' });
    }
  }, [entries]);

  const run = async (method: string, args: (ids: string[]) => Record<string, unknown>, count: number): Promise<OperationStatus | null> => {
    if (!domain.current) return null;
    setBusy(true); setError(''); setNotice(''); setUnknown(null);
    const controller = new AbortController(); abort.current = controller;
    let knownIds: string[] = []; let beginSent = false;
    try {
      const ids = await domain.current.allocate(count);
      knownIds = ids;
      const parameters = args(ids);
      beginSent = true;
      const operation = await domain.current.begin(method, parameters);
      if (!mounted.current || controller.signal.aborted) { void domain.current.release(operation).catch(() => {}); return null; }
      active.current = operation;
      const result = await domain.current.wait(operation, controller.signal);
      active.current = null;
      if (result.status === 'unknown') { setUnknown(result.itemIds); setNotice('The outcome is unknown. Reread the World before trying again.'); }
      if (result.status === 'failed') setError(result.reason.includes('stale-entry-id') ? 'A newer message was saved first. Press Send again to retry with new IDs.' : result.reason);
      return result;
    } catch (failure) {
      if (mounted.current) {
        const notStarted = (failure instanceof DomainError || failure instanceof BridgeError) && failure.outcome === 'not-started';
        if (beginSent && (active.current || !notStarted)) {
          setUnknown(active.current?.itemIds ?? knownIds);
          setNotice('The outcome is unknown. Reread the World before trying again.');
        }
        setError(message(failure));
      }
      return null;
    } finally { abort.current = null; if (mounted.current) setBusy(false); }
  };
  const create = async () => {
    if (!title.trim() || new TextEncoder().encode(title).length > 256) { setError('Enter a title of at most 256 UTF-8 bytes.'); return; }
    const result = await run('begin-create', ids => ({ conversationId: ids[0], title, ...(characterId ? { characterId } : {}) }), 1);
    if (result?.status !== 'committed' || !mounted.current) return;
    saveState({ conversationId: result.itemIds[0] });
    setTitle(''); setCreateOpen(false);
    try { await loadList(); setPageConversation(null); setSelected(result.itemIds[0]); await loadLatest(result.itemIds[0]); } catch (failure) { setError(message(failure)); }
  };
  const generate = async (conversationId: string, userMessageId: string) => {
    if (!narrator.current || !domain.current || !bridge.current) return;
    const owner = narrator.current;
    const controller = new AbortController(); aiAbort.current = controller;
    const current = () => mounted.current && aiAbort.current === controller && !controller.signal.aborted && selectedRef.current === conversationId;
    let operation: Operation | null = null, knownIds: string[] = [], beginSent = false;
    const markPreview = (phase: ReplyPreview['phase']) => setReplyPreview(previous => previous?.conversationId === conversationId ? { ...previous, phase } : previous);
    setAiBusy(true); setReplyPreview(null); setError(''); setNotice('');
    try {
      const info = await new DomainClient(bridge.current, 'rintawa/inference/v1').request<{ ready: boolean; reason: string | null }>('describe', {}, controller.signal);
      if (!current()) return;
      if (!info.ready) throw new Error(aiFailure(info.reason ?? 'The selected AI Provider is not ready.'));
      knownIds = await domain.current.allocate(2);
      if (!current()) return;
      beginSent = true;
      operation = await owner.begin('begin-generate', { conversationId, userMessageId, messageId: knownIds[0], entryId: knownIds[1] });
      aiOperation.current = operation;
      if (!current()) return;
      followingReply.current = !transcript.current || transcript.current.scrollHeight - transcript.current.scrollTop - transcript.current.clientHeight < 64;
      setReplyPreview({ conversationId, messageId: knownIds[0], text: '', phase: 'streaming' });
      const result = await owner.wait(operation, controller.signal, text => {
        if (!current()) return;
        const view = transcript.current;
        followingReply.current = !view || view.scrollHeight - view.scrollTop - view.clientHeight < 64;
        setReplyPreview({ conversationId, messageId: knownIds[0], text, phase: 'streaming' });
      });
      if (!current()) return;
      if (result.status === 'committed') {
        markPreview('saved');
        await loadLatest(conversationId).catch(failure => readFailure('transcript', failure));
      } else if (result.status === 'failed') { markPreview('interrupted'); setError(aiFailure(result.reason)); }
      else { markPreview('unknown'); setUnknown(result.itemIds); setNotice('Reply outcome unknown. Reread the World before trying again.'); }
    } catch (failure) {
      if (current()) {
        markPreview('interrupted');
        const notStarted = (failure instanceof DomainError || failure instanceof BridgeError) && failure.outcome === 'not-started';
        if (beginSent && (operation || !notStarted)) { markPreview('unknown'); setUnknown(knownIds); setNotice('Reply outcome unknown. Reread the World before trying again.'); }
        setError(message(failure));
      }
    } finally {
      if (operation) void owner.release(operation).catch(() => {});
      if (aiAbort.current === controller) {
        aiOperation.current = null; aiAbort.current = null;
        if (mounted.current) setAiBusy(false);
      }
    }
  };
  const send = async () => {
    if (!selected) return;
    if (!text.trim() || new TextEncoder().encode(text).length > 16 * 1024) { setError('Enter a message of at most 16 KiB in UTF-8.'); return; }
    const id = selected;
    const result = await run('begin-send', ids => ({ conversationId: id, messageId: ids[0], entryId: ids[1], text }), 2);
    if (result?.status !== 'committed' || !mounted.current) return;
    setText('');
    try { await loadLatest(id); } catch (failure) { setError(message(failure)); }
    let available = aiAvailable;
    if (!available && narrator.current) {
      try { available = (await narrator.current.request<{ available: boolean }>('describe')).available; if (mounted.current) setAiAvailable(available); } catch { /* Manual Chat remains usable without Narrator. */ }
    }
    if (mounted.current && available) await generate(id, result.itemIds[0]);
  };
  const read = async (action: () => Promise<void>) => {
    setBusy(true); setError('');
    try { await action(); } catch (failure) { if (mounted.current) setError(message(failure)); }
    finally { if (mounted.current) setBusy(false); }
  };
  const select = (id: string) => { saveState({ conversationId: id }); setSelected(id); setPageConversation(null); setEntries([]); setCursor(null); void read(() => loadLatest(id)); };
  const shown = conversations.filter(row => row.conversation.title.toLowerCase().includes(search.toLowerCase()));
  const newConversation = () => { setCreateOpen(true); void loadCards().catch(() => {}); };
  const preview = replyPreview?.conversationId === selected && !entries.some(entry => entry.messageId === replyPreview.messageId) ? replyPreview : null;
  return <><SplitPane className={`chat${navigationOpen ? '' : ' navigation-closed'}`} open={navigationOpen} ratio={navigationRatio} onResize={resizeNavigation} sidebarId="conversation-navigation" label="Conversation list width"
    onCollapse={() => { toggleNavigation(false); requestAnimationFrame(() => navigationToggle.current?.focus()); }} sidebar={<aside id="conversation-navigation" className="conversation-sidebar">
      <header className="conversation-topbar"><h1>Chats</h1></header>
      <div className="conversation-search"><Icon name="search" size={15} /><input aria-label="Find a conversation" value={search} onChange={event => setSearch(event.target.value)} placeholder="Find a conversation" /></div>
      <div className="channel-section"><span>CONVERSATIONS</span><button className="icon-button" title="New conversation" aria-label="New conversation" disabled={!ready || busy || !!unknown} onClick={newConversation}><Icon name="plus" size={17} /><span className="sr-only">New conversation</span></button></div>
      <nav className="channel-list" aria-label="Conversations">{shown.map(row => <button key={row.id} aria-current={row.id === selected ? 'page' : undefined} className={row.id === selected ? 'conversation selected' : 'conversation'} disabled={busy} onClick={() => select(row.id)}><Icon name="hash" size={19} /><span>{row.conversation.title}</span></button>)}{!shown.length && ready && <p className="channels-empty">{search ? 'No matching conversations.' : 'No conversations yet.'}</p>}{listCursor && <button className="more-conversations" disabled={busy} onClick={() => void read(() => loadList(listCursor))}>More conversations</button>}</nav>
      <button className="new-conversation-button" disabled={!ready || busy || !!unknown} onClick={newConversation}><Icon name="plus" size={17} /><span>New conversation</span></button>

    </aside>}>
    <section className="thread" aria-label="Conversation">
      <header className="thread-header"><div className="chat-heading">{portrait ? <img src={portrait} alt={linked?.card.name ?? ''} /> : <span className="header-hash"><Icon name="hash" size={24} /></span>}<div><h2>{selectedConversation?.title ?? 'Conversations'}</h2>{linked && <small>{linked.card.name}</small>}</div></div><div className="thread-actions"><button className="icon-button navigation-toggle" title={navigationOpen ? 'Hide conversations' : 'Show conversations'} aria-label="Toggle conversations" aria-controls="conversation-navigation" aria-expanded={navigationOpen} disabled={!ready} ref={navigationToggle} onClick={() => toggleNavigation(!navigationOpen)}><Icon name="split" size={18} /></button></div></header>
      <div className="transcript" ref={transcript} aria-live="polite">{selected ? <div className="message-column">{cursor && <Button className="load-more" disabled={busy} onClick={() => void read(() => loadOlder(selected, cursor))}>Load older messages</Button>}<div className="conversation-start"><span className="conversation-start-icon"><Icon name="hash" size={31} /></span><h2>{selectedConversation?.title ?? 'Conversation'}</h2><p>{linked ? `A conversation with ${linked.card.name}.` : 'This is the beginning of your conversation.'}</p></div>{entries.map(entry => <article className="message" key={entry.entryId}><div className={`message-avatar ${entry.message.role}`}>{entry.message.role === 'assistant' && portrait ? <img src={portrait} alt="" /> : <Icon name="user" size={19} />}</div><div className="message-body"><strong>{entry.message.role === 'user' ? 'You' : linked?.card.name ?? 'Assistant'}</strong><p>{entry.message.text}</p>{!!messageActions.items.length && <div className="message-actions" aria-label="Message actions">{messageActions.items.map(action=><Button key={action.key} disabled={messageActions.busy} onClick={()=>void messageActions.invoke(action,{conversationId:selected!,messageId:entry.messageId,expectedRevision:entry.message.revision})}>{action.label}</Button>)}</div>}</div></article>)}{preview && (preview.text || preview.phase === 'streaming') && <article className={`message reply-preview ${preview.phase}`} data-message-id={preview.messageId} aria-label="Assistant reply preview" aria-busy={preview.phase === 'streaming'}><div className="message-avatar assistant">{portrait ? <img src={portrait} alt="" /> : <Icon name="user" size={19} />}</div><div className="message-body"><strong>{linked?.card.name ?? 'Assistant'}</strong>{preview.text ? <p>{preview.text}{preview.phase === 'streaming' && <span className="reply-cursor" aria-hidden="true" />}</p> : preview.phase === 'streaming' ? <span className="reply-typing" role="status" aria-label="Generating reply"><i /><i /><i /></span> : null}</div></article>}</div> : <div className="chat-empty"><div className="chat-empty-icon"><Icon name="chat" size={34} /></div><h2>A place for your next story.</h2><p>Choose a conversation to continue.</p><Button className="primary" disabled={!ready || busy} onClick={() => toggleNavigation(true)}>Conversations</Button></div>}</div>
      <div className="thread-notices">{messageActions.error && <Status error>{messageActions.error}</Status>}{messageActions.notice && <Status>{messageActions.notice}</Status>}{Object.values(readErrors).some(Boolean) && <div><Status error>Saved data may be out of date. {Object.values(readErrors).filter(Boolean).join(' · ')}</Status><Button onClick={() => { void listWatch.current?.refresh().catch(() => {}); void cardsWatch.current?.refresh().catch(() => {}); void threadWatch.current?.refresh().catch(() => {}); }}>Retry reads</Button></div>}{!ready && !error && <Status>Connecting…</Status>}{error && !unknown && <div className="reply-error"><Status error>{error}</Status>{aiAvailable && !aiBusy && selected && entries.at(-1)?.message.role === 'user' && <Button disabled={busy} onClick={() => void generate(selected, entries.at(-1)!.messageId)}>Retry reply</Button>}</div>}{notice && <Status>{notice}</Status>}{unknown && <div className="unknown"><p>Known item IDs: {unknown.join(', ')}</p><Button disabled={busy} onClick={() => void read(async () => { await loadList(); if (selected) await loadLatest(selected); if (active.current) { void domain.current?.release(active.current).catch(() => {}); active.current = null; } setUnknown(null); setNotice('World data reread. An absent item does not prove rollback; retry only by an explicit decision.'); })}>Reread World</Button></div>}</div>
      {selected && <div className="composer-area"><div className="composer" role="form" aria-label="Send a message"><textarea aria-label="Message" ref={composer} id="message" value={text} onChange={event => setText(event.target.value)} placeholder={busy ? '' : 'Message…'} disabled={busy} rows={1} onKeyDown={event => { if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); if (!busy && !unknown && text.trim()) void send(); } }} /><ComposerActions client={bridge.current}/><button className="send-button" type="button" onClick={() => { if (!busy && !unknown && text.trim()) void send(); }} title="Send" aria-label="Send" disabled={busy || !ready || !!unknown || !text.trim()}><Icon name="send" size={18} /><span className="sr-only">Send</span></button></div></div>}
    </section>
    </SplitPane>
    <Dialog open={createOpen} title="New conversation" onClose={() => { if (!busy) setCreateOpen(false); }}><p className="dialog-description">Give this conversation a name. Add a character if you like.</p><div className="dialog-form" role="form" aria-label="New conversation"><label htmlFor="new-title">CONVERSATION NAME</label><Input id="new-title" value={title} onChange={event => setTitle(event.target.value)} placeholder="A new beginning" disabled={busy} onKeyDown={event => { if (event.key === 'Enter' && !event.nativeEvent.isComposing) { event.preventDefault(); if (!busy && !unknown && title.trim()) void create(); } }} /><label htmlFor="character">CHARACTER · OPTIONAL</label><div className="character-picker"><select id="character" value={characterId} disabled={busy} onChange={event => setCharacterId(event.target.value)}><option value="">No character</option>{cards.map(card => <option key={card.id} value={card.id}>{card.card.name}</option>)}</select></div>{cardCursor && <Button disabled={busy} onClick={() => void read(() => loadCards(cardCursor))}>More characters</Button>}{error && <Status error>{error}</Status>}{unknown && <Status>Outcome unknown. Close this dialog and reread the World before another mutation.</Status>}<footer><Button disabled={busy} onClick={() => setCreateOpen(false)}>Cancel</Button><Button className="primary" onClick={() => void create()} disabled={busy || !ready || !!unknown || !title.trim()}>Create</Button></footer></div></Dialog>
  </>;
}
createRoot(document.getElementById('root')!).render(<Chat />);
