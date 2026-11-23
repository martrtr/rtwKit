import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, type Client } from '@rintawa/bridge';
import { Button, Status } from '@rintawa/ui';
import { viewContributions, type ViewContribution } from '../../../web/ui/src/contributions';
import '@rintawa/ui/style.css';
import './style.css';
function Section({ client, item }: { client: Client; item: ViewContribution | null }) {
  const element = useRef<HTMLDivElement>(null); const [error, setError] = useState(''); const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!item || !element.current) return;
    let disposed = false, mount: { close: () => Promise<void> } | undefined; setError('');
    client.mountView(item.view, element.current, {}, () => { if (!disposed) setError('Section unavailable.'); }).then(value => { if (disposed) void value.close().catch(() => {}); else mount = value; }).catch(error => { if (!disposed) setError(error instanceof Error ? error.message : 'Section unavailable.'); });
    return () => { disposed = true; void mount?.close().catch(() => {}); };
  }, [client, item?.id, item?.view.activation, attempt]);
  return <section className="settings-content"><div className="settings-section-slot" ref={element} />{error && <div><Status error>{error}</Status>{item && <Button onClick={()=>setAttempt(v=>v+1)}>Reconnect section</Button>}</div>}{!item && <Status>No active settings sections.</Status>}</section>;
}
function App() {
  const [client, setClient] = useState<Client | null>(null), [items, setItems] = useState<ViewContribution[]>([]), [selected, setSelected] = useState(''), [error, setError] = useState('');
  const refresh = useRef<() => void>(() => {});
  useEffect(() => {
    let disposed = false, active: Client | undefined, timer: ReturnType<typeof setTimeout> | undefined;
    connectPanel().then(value => {
      if (disposed) { value.dispose(); return; } active = value; setClient(value);
      let reading = false;
      const read = async () => { if (disposed || reading) return; reading = true; try { const next = await viewContributions(value, 'rintawa/ui/settings/sections/v1', 'title'); if (!disposed) { setItems(next.items); setError(next.rejected ? 'Some settings contributions are invalid or ambiguous.' : ''); } } catch (error) { if (!disposed) setError(error instanceof Error ? error.message : 'Settings unavailable.'); } finally { reading = false; if (!disposed) timer = setTimeout(() => void read(), 1500); } };
      refresh.current = () => { clearTimeout(timer); void read(); }; void read();
    }).catch(error => { if (!disposed) setError(error.message); });
    return () => { disposed = true; clearTimeout(timer); active?.dispose(); };
  }, []);
  const item = items.find(item => item.id === selected) ?? items[0] ?? null;
  return <main className="settings-layout"><aside className="settings-navigation" aria-label="Settings sections">{items.map(section => <button key={section.id} aria-current={item?.id === section.id ? 'page' : undefined} onClick={() => setSelected(section.id)}>{section.label}</button>)}<button className="settings-refresh" aria-label="Refresh settings" onClick={() => refresh.current()}>↻</button>{error && <Status error>{error}</Status>}</aside>{client && <Section client={client} item={item} />}</main>;
}
createRoot(document.getElementById('root')!).render(<App />);
