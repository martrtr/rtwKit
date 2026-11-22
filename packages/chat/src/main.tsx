import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, BridgeError, type Client } from '@rintawa/bridge';
import { DomainClient, DomainError, type Operation, type OperationStatus } from '@rintawa/domain';
import { Button, Input, Status } from '@rintawa/ui';
import '@rintawa/ui/style.css';
import './style.css';

type Conversation = { title: string; characterId: string | null; lastEntryId: string | null };
type ConversationRow = { id: string; conversation: Conversation };
type Entry = { entryId: string; messageId: string; message: { role: string; text: string } };
type Page = { conversation: Conversation; items: Entry[]; next: string | null };
type ConversationList = { items: ConversationRow[]; next: string | null };
const message = (error: unknown) => error instanceof Error ? error.message : 'Request unavailable';

function Chat() {
  const bridge = useRef<Client | null>(null);
  const domain = useRef<DomainClient | null>(null);
  const active = useRef<Operation | null>(null);
  const abort = useRef<AbortController | null>(null);
  const mounted = useRef(true);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [unknown, setUnknown] = useState<string[] | null>(null);
  const [conversations, setConversations] = useState<ConversationRow[]>([]);
  const [listCursor, setListCursor] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [entries, setEntries] = useState<Entry[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [title, setTitle] = useState('');
  const [text, setText] = useState('');

  const loadList = async (after?: string) => {
    const result = await domain.current!.request<ConversationList>('list', after ? { after } : {});
    if (!mounted.current) return;
    setConversations(previous => after ? [...previous, ...result.items] : result.items);
    setListCursor(result.next);
  };
  const loadPage = async (id: string, after?: string) => {
    const result = await domain.current!.request<Page>('page', { conversationId: id, ...(after ? { after } : {}) });
    if (!mounted.current) return;
    setEntries(previous => after ? [...previous, ...result.items] : result.items);
    setCursor(result.next);
  };
  useEffect(() => {
    mounted.current = true;
    connectPanel().then(async client => {
      if (!mounted.current) { client.dispose(); return; }
      bridge.current = client;
      domain.current = new DomainClient(client, 'rintawa/chat/v1');
      try { await loadList(); } catch (failure) { if (mounted.current) setError(message(failure)); }
      finally { if (mounted.current) setReady(true); }
    }).catch(failure => { if (mounted.current) setError(message(failure)); });
    return () => {
      mounted.current = false;
      abort.current?.abort();
      if (active.current && domain.current) void domain.current.release(active.current).catch(() => {});
      bridge.current?.dispose();
    };
  }, []);

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
    const result = await run('begin-create', ids => ({ conversationId: ids[0], title }), 1);
    if (result?.status !== 'committed' || !mounted.current) return;
    setTitle('');
    try { await loadList(); setSelected(result.itemIds[0]); await loadPage(result.itemIds[0]); } catch (failure) { setError(message(failure)); }
  };
  const send = async () => {
    if (!selected) return;
    if (!text.trim() || new TextEncoder().encode(text).length > 16 * 1024) { setError('Enter a message of at most 16 KiB in UTF-8.'); return; }
    const id = selected;
    const result = await run('begin-send', ids => ({ conversationId: id, messageId: ids[0], entryId: ids[1], text }), 2);
    if (result?.status !== 'committed' || !mounted.current) return;
    setText('');
    try { await loadPage(id); } catch (failure) { setError(message(failure)); }
  };
  const read = async (action: () => Promise<void>) => {
    setBusy(true); setError('');
    try { await action(); } catch (failure) { if (mounted.current) setError(message(failure)); }
    finally { if (mounted.current) setBusy(false); }
  };
  const select = (id: string) => { setSelected(id); setEntries([]); setCursor(null); void read(() => loadPage(id)); };
  return <main className="chat">
    <aside className="sidebar"><h1>Chat</h1><label htmlFor="new-title">New conversation</label><Input id="new-title" value={title} onChange={event => setTitle(event.target.value)} placeholder="Conversation title" disabled={!ready || busy} /><Button disabled={!ready || busy || !!unknown} onClick={() => void create()}>Create</Button><h2>Conversations</h2><Button disabled={!ready || busy} onClick={() => void read(() => loadList())}>Refresh conversations</Button><nav aria-label="Conversations">{conversations.map(row => <button key={row.id} className={selected === row.id ? 'conversation selected' : 'conversation'} disabled={busy} onClick={() => select(row.id)}>{row.conversation.title}</button>)}</nav>{listCursor && <Button disabled={busy} onClick={() => void read(() => loadList(listCursor))}>More conversations</Button>}</aside>
    <section className="workspace" aria-label="Conversation"><header><h2>{conversations.find(row => row.id === selected)?.conversation.title ?? 'Select a conversation'}</h2>{selected && <Button disabled={busy} onClick={() => void read(() => loadPage(selected))}>Refresh</Button>}</header><div className="transcript" aria-live="polite">{entries.map(entry => <article className="message" key={entry.entryId}><strong>{entry.message.role}</strong><p>{entry.message.text}</p></article>)}{cursor && selected && <Button disabled={busy} onClick={() => void read(() => loadPage(selected, cursor))}>Load more messages</Button>}</div>{selected && <div className="composer"><label htmlFor="message">Message</label><textarea id="message" value={text} onChange={event => setText(event.target.value)} disabled={busy} rows={3} /><Button disabled={busy || !ready || !!unknown} onClick={() => void send()}>Send</Button></div>}{!ready && !error && <Status>Connecting…</Status>}{busy && <Status>Working…</Status>}{error && <Status error>{error}</Status>}{notice && <Status>{notice}</Status>}{unknown && <div className="unknown"><p>Known item IDs: {unknown.join(', ')}</p><Button disabled={busy} onClick={() => void read(async () => { await loadList(); if (selected) await loadPage(selected); if (active.current) { void domain.current?.release(active.current).catch(() => {}); active.current = null; } setUnknown(null); setNotice('World data reread. An absent item does not prove rollback; retry only by an explicit decision.'); })}>Reread World</Button></div>}</section>
  </main>;
}
createRoot(document.getElementById('root')!).render(<Chat />);
