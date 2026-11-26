import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, BridgeError, type Client } from '@rintawa/bridge';
import { DomainClient, DomainError, type Operation } from '@rintawa/domain';
import { Button, Status } from '@rintawa/ui';
import '@rintawa/ui/style.css';
import './style.css';

type Kind = 'location' | 'scene';
type RecordData = { revision: string; name: string; description: string; archived: boolean };
type Summary = Omit<RecordData, 'description'> & { id: string; locationId?: string };
type Item = Summary & { description: string };
type Page = { items: Summary[]; next: string | null };
type Editor = { kind: Kind; item?: Item; record: RecordData; locationId: string };
type Uncertain = { kind: Kind; id: string; relationId?: string };
const empty = (): RecordData => ({ revision: '1', name: '', description: '', archived: false });
const message = (e: unknown) => e instanceof Error ? e.message : 'Request unavailable';
const methods = { location: 'locations', scene: 'scenes' } as const;

function App() {
  const client = useRef<Client | null>(null);
  const domain = useRef<DomainClient | null>(null);
  const alive = useRef(true); const active = useRef<Operation | null>(null);
  const controller = useRef<AbortController | null>(null);
  const [ready, setReady] = useState(false), [busy, setBusy] = useState(false);
  const [locations, setLocations] = useState<Summary[]>([]), [scenes, setScenes] = useState<Summary[]>([]);
  const [more, setMore] = useState<Record<Kind, string | null>>({location:null,scene:null});
  const [archived, setArchived] = useState(false), [editor, setEditor] = useState<Editor | null>(null);
  const [notice, setNotice] = useState(''), [error, setError] = useState('');
  const [unknown, setUnknown] = useState<Uncertain | null>(null);
  // Load a bounded page window. Additional pages are an explicit user action.
  const windowSize = useRef<Record<Kind, number>>({ location: 8, scene: 8 });
  async function refresh() {
    const d = domain.current!;
    for (const kind of ['location','scene'] as const) {
      const items: Summary[] = []; let next: string | null = null;
      for (let page=0; page<windowSize.current[kind]; page++) {
        const result: Page = await d.request<Page>('list-'+methods[kind], next ? {after:next} : {});
        if (!alive.current) return;
        items.push(...result.items); next=result.next;
        if (!next) break;
      }
      if (!alive.current) return;
      (kind==='location'?setLocations:setScenes)(items);
      setMore(previous=>({...previous,[kind]:next}));
    }
  }
  function show(kind: Kind, item: Item) {
    setEditor({kind,item,record:{name:item.name,description:item.description,archived:item.archived,revision:item.revision},locationId:item.locationId ?? ''});
  }
  useEffect(()=>{
    alive.current=true;
    void connectPanel().then(async c=>{
      if (!alive.current) {c.dispose();return;}
      client.current=c; domain.current=new DomainClient(c,'rintawa/world-atlas/v1');
      await refresh();
      if (alive.current) setReady(true);
    }).catch(e=>{if(alive.current)setError(message(e));});
    return ()=>{
      alive.current=false;controller.current?.abort();
      const d=domain.current,c=client.current, op=active.current;
      // A pending begin owns its eventual operation cleanup; do not dispose its
      // transport before the reply can be released.
      if (op) void d?.release(op).catch(()=>{}).finally(()=>c?.dispose());
      else if (!controller.current) c?.dispose();
    };
  },[]);
  async function read(action:()=>Promise<void>) {
    setBusy(true);setError('');
    try{await action();}catch(e){if(alive.current)setError(message(e));}
    finally{if(alive.current)setBusy(false);}
  }
  async function open(kind:Kind,id:string) {
    await read(async()=>{const item=await domain.current!.request<Item>('get-'+kind,{itemId:id});if(alive.current)show(kind,item);});
  }
  function create(kind:Kind) {
    setEditor({kind,record:empty(),locationId:locations.find(l=>!l.archived)?.id ?? ''});
    setError('');setNotice('');
  }
  async function submit(move=false) {
    const draft=editor,d=domain.current; if(!draft||!d||busy||unknown)return;
    const {kind,item,record,locationId}=draft;
    setBusy(true);setError('');setNotice('');
    const abort=new AbortController();controller.current=abort;
    let id=item?.id;let relationId:string|undefined;
    let beginSent=false;let terminal=false;
    try {
      if(!item){const ids=await d.allocate(kind==='scene'?2:1); id=ids[0];relationId=ids[1];}
      else if(move){[relationId]=await d.allocate(1);}
      if(!alive.current)return;
      const method=move?'begin-move-scene':item?'begin-update-'+kind:'begin-create-'+kind;
      const expectedRevision=item?.revision;
      const args:Record<string,unknown>={itemId:id};
      if(item)args.expectedRevision=expectedRevision;
      if(!move)args.record={...record,revision:item?(BigInt(item.revision)+1n).toString():'1'};
      if(relationId){args.relationId=relationId;args.locationId=locationId;}
      beginSent=true;
      const operation=await d.begin(method,args);
      if(!alive.current){await d.release(operation).catch(()=>{});return;}
      active.current=operation;
      const outcome=await d.wait(operation,abort.signal);
      active.current=null;
      if(!alive.current)return;
      if(outcome.status==='unknown'){setUnknown({kind,id:id!,relationId});setNotice('Outcome unknown. Reread the known ID before another edit.');return;}
      terminal=true;
      if(outcome.status==='failed'){setError(outcome.reason);return;}
      setNotice('Saved.');
      const saved=await d.request<Item>('get-'+kind,{itemId:id});
      if(alive.current)show(kind,saved);
      await refresh();
    }catch(e){
      if(alive.current){
        const notStarted=(e instanceof DomainError||e instanceof BridgeError)&&e.outcome==='not-started';
        if(id&&beginSent&&!terminal&&(active.current||!notStarted)){setUnknown({kind,id,relationId});setNotice('Outcome unknown. Reread the known ID before another edit.');}
        setError(message(e));
      }
    }finally{
      controller.current=null;
      if(!alive.current)client.current?.dispose();
      else setBusy(false);
    }
  }
  const blocked=!ready||busy||!!unknown;
  const visible=(items:Summary[])=>items.filter(i=>archived||!i.archived);
  const locationName=(id?:string)=>locations.find(l=>l.id===id)?.name ?? id ?? '';
  return <main className="world-atlas">
    <header><h1>World Atlas</h1><label><input type="checkbox" checked={archived} onChange={e=>setArchived(e.target.checked)}/> Show archived</label><Button disabled={busy||!domain.current} onClick={()=>void read(async()=>{await refresh();if(alive.current)setReady(true);})}>Refresh</Button></header>
    {!ready&&!error&&<Status>Connecting…</Status>}{busy&&<Status>Working…</Status>}{error&&<Status error>{error}</Status>}{notice&&<Status>{notice}</Status>}
    {unknown&&<section aria-label="Unknown outcome"><p>Known item: {unknown.id}{unknown.relationId&&<> · relation: {unknown.relationId}</>}</p><Button disabled={busy} onClick={()=>void read(async()=>{
      const result=await domain.current!.request<Item>('get-'+unknown.kind,{itemId:unknown.id});
      if(alive.current){show(unknown.kind,result);if(active.current)void domain.current!.release(active.current).catch(()=>{});active.current=null;setUnknown(null);setNotice('Canonical record reread. Verify its fields before editing.');}
      await refresh();
    })}>Reread item</Button><Button disabled={busy} onClick={()=>{if(active.current)void domain.current?.release(active.current).catch(()=>{});active.current=null;setUnknown(null);setEditor(null);setNotice('Absence does not prove rollback. Start another edit only by explicit decision.');}}>Allow another edit</Button></section>}
    <div className="world-atlas-body">
      <nav aria-label="Locations and scenes">
        {(['location','scene'] as const).map(kind=><section key={kind}>
          <h2>{kind==='location'?'Locations':'Scenes'} <Button disabled={blocked||(kind==='scene'&&!locations.some(l=>!l.archived))} onClick={()=>create(kind)}>New {kind}</Button></h2>
          {visible(kind==='location'?locations:scenes).map(i=><button className={editor?.item?.id===i.id?'row selected':'row'} key={i.id} disabled={blocked} onClick={()=>void open(kind,i.id)}><strong>{i.name}</strong><small>{i.archived?'Archived':'Active'}{kind==='scene'?' · '+locationName(i.locationId):''}</small></button>)}
          {more[kind]&&<Button disabled={busy} onClick={()=>void read(async()=>{windowSize.current[kind]+=8;await refresh();})}>More {methods[kind]}</Button>}
        </section>)}
      </nav>
      <section className="editor" aria-label="World Atlas editor">
        {editor?<><h2>{editor.item?'Edit':'New'} {editor.kind}</h2>{editor.item&&<small>Revision {editor.item.revision} · {editor.item.id}</small>}
          <label>Name<input value={editor.record.name} disabled={blocked} onChange={e=>setEditor({...editor,record:{...editor.record,name:e.target.value}})}/></label>
          <label>Description<textarea value={editor.record.description} disabled={blocked} onChange={e=>setEditor({...editor,record:{...editor.record,description:e.target.value}})}/></label>
          <label><input type="checkbox" checked={editor.record.archived} disabled={blocked} onChange={e=>setEditor({...editor,record:{...editor.record,archived:e.target.checked}})}/> Archived</label>
          {editor.kind==='scene'&&<><p>Current location: {locationName(editor.item?.locationId)}</p><label>{editor.item?'Move to':'Location'}<select value={editor.locationId} disabled={blocked||!!editor.item?.archived} onChange={e=>setEditor({...editor,locationId:e.target.value})}>
            {!locations.some(l=>l.id===editor.locationId)&&editor.locationId&&<option value={editor.locationId}>{editor.locationId}</option>}
            <option value="">Select location</option>{locations.filter(l=>!l.archived||l.id===editor.item?.locationId).map(l=><option key={l.id} value={l.id} disabled={l.archived}>{l.name}{l.archived?' (Archived)':''}</option>)}</select></label></>}
          <div className="actions"><Button disabled={blocked||!editor.record.name.trim()||(!editor.item&&editor.kind==='scene'&&!editor.locationId)} onClick={()=>void submit()}>Save</Button>
          {editor.kind==='scene'&&editor.item&&<Button disabled={blocked||editor.item.archived||!editor.locationId||editor.locationId===editor.item.locationId} onClick={()=>void submit(true)}>Move scene</Button>}<Button disabled={blocked} onClick={()=>setEditor(null)}>Close</Button></div>
        </>:<p>Select an item or create a location or scene.</p>}
      </section>
    </div>
  </main>;
}
createRoot(document.getElementById('root')!).render(<App/>);
