import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, type Client } from '@rintawa/bridge';
import { observeResource, type Observation } from '@rintawa/domain';
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
  const watch=useRef<Observation|null>(null);
  useEffect(() => {
    let disposed=false,active:Client|undefined;
    connectPanel().then(value=>{
      if(disposed){value.dispose();return;}active=value;setClient(value);
      watch.current=observeResource({client:value,read:signal=>viewContributions(value,'rintawa/ui/settings/sections/v1','title',signal),
        apply:next=>{setItems(next.items);setError(next.rejected?'Some settings contributions are invalid or ambiguous.':'');},
        failed:error=>setError(error instanceof Error?error.message:'Settings unavailable.')});
    }).catch(error=>{if(!disposed)setError(error.message);});
    return ()=>{disposed=true;watch.current?.close();active?.dispose();};
  }, []);
  const item = items.find(item => item.id === selected) ?? items[0] ?? null;
  return <main className="settings-layout"><aside className="settings-navigation" aria-label="Settings sections">{items.map(section => <button key={section.id} aria-current={item?.id === section.id ? 'page' : undefined} onClick={() => setSelected(section.id)}>{section.label}</button>)}<>{error && <div><Status error>{error}</Status><Button onClick={()=>void watch.current?.refresh().catch(()=>{})}>Retry reads</Button></div>}</></aside>{client && <Section client={client} item={item} />}</main>;
}
createRoot(document.getElementById('root')!).render(<App />);
