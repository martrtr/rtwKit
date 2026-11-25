import { useEffect, useRef, useState } from 'react';
import type { Client, ViewState } from '@rintawa/bridge';
import { observeResource } from '@rintawa/domain';
import { Button, Status } from '@rintawa/ui';
import { viewContributions, type ViewContribution } from '../../../web/ui/src/contributions';

export type MessageViewContext = { conversationId: string; entryId: string; messageId: string; expectedRevision: string; conversationRevision: string };
type Expansion = { key: string; item: ViewContribution; context: MessageViewContext };
const identity = (item: ViewContribution) => JSON.stringify([item.id, item.view.activation]);
export function useMessageViews(client: Client | null, conversationId: string | null, revision: string | undefined) {
  const [items, setItems] = useState<ViewContribution[]>([]);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [expanded, setExpanded] = useState<Expansion[]>([]);
  const current = useRef(expanded); current.current = expanded;
  const close = (key: string) => { current.current = current.current.filter(value => value.key !== key); setExpanded(current.current); setNotice(''); };
  useEffect(() => {
    current.current = []; setExpanded([]); setNotice('');
  }, [client, conversationId, revision]);
  useEffect(() => {
    if (!client) return;
    const watch = observeResource({ client,
      read: signal => viewContributions(client, 'rintawa/chat/message-views/v1', 'label', signal),
      apply: value => {
        setItems(value.items); setError(value.rejected ? 'Some message views are invalid or ambiguous.' : '');
        const live = new Set(value.items.map(identity));
        current.current = current.current.filter(value => live.has(identity(value.item)));
        setExpanded(current.current);
      }, failed: failure => setError(failure instanceof Error ? failure.message : 'Message views unavailable.'),
    });
    return () => watch.close();
  }, [client]);
  const open = (item: ViewContribution, context: MessageViewContext) => {
    const key = JSON.stringify([context.entryId, identity(item)]);
    if (current.current.some(value => value.key === key)) { close(key); return; }
    if (current.current.length >= 4) { setNotice('Close an open message view before opening another (maximum four).'); return; }
    current.current = [...current.current, { key, item, context }]; setExpanded(current.current); setNotice('');
  };
  return { items, expanded, open, close, error, notice };
}

export function MessageView({ client, expansion, onClose }: { client: Client; expansion: Expansion; onClose: () => void }) {
  const container = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose); closeRef.current = onClose;
  const [error, setError] = useState('');
  useEffect(() => {
    const element = container.current!;
    let disposed = false, started = false;
    let binding: { close: () => Promise<void> } | undefined;
    const observer = new IntersectionObserver(entries => {
      if (disposed) return;
      if (!entries.some(entry => entry.isIntersecting)) { closeRef.current(); return; }
      if (started) return;
      started = true;
      void client.mountView(expansion.item.view, element, expansion.context as ViewState, () => {
        if (!disposed && binding) closeRef.current();
      }).then(value => {
        if (disposed) { void value.close().catch(() => {}); return; }
        binding = value;
      }).catch(failure => { if (!disposed) setError(failure instanceof Error ? failure.message : 'Message view unavailable.'); });
    }, { root: element.closest('.transcript') });
    observer.observe(element.closest('article.message') ?? element);
    return () => { disposed = true; observer.disconnect(); void binding?.close().catch(() => {}); };
  }, [client, expansion]);
  return <section className="message-view" aria-label={expansion.item.label}>
    <header><span>{expansion.item.label}</span><Button aria-label={`Close ${expansion.item.label}`} onClick={onClose}>Close</Button></header>
    {error && <Status error>{error}</Status>}
    <div className="message-view-slot" ref={container} />
  </section>;
}
