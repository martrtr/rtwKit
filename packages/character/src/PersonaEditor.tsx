import { useEffect, useRef, useState } from 'react';
import type { AssetRef, Client } from '@rintawa/bridge';
import { DomainClient, type Operation, observeResource } from '@rintawa/domain';
import { Button, Status } from '@rintawa/ui';

type Persona = { revision: string; name: string; description: string; portrait: AssetRef | null };
type Row = { id: string; persona: Omit<Persona, 'description'> };
type Page = { items: Row[]; next: string | null };
const blank = (): Persona => ({ revision: '0', name: '', description: '', portrait: null });
export function PersonaEditor({ client, selection, onSelection, busy = false }: { client: Client; selection?: string | null; onSelection?: (id: string | null) => void; busy?: boolean }) {
  const domain = useRef(new DomainClient(client, 'rintawa/character/v1'));
  const [rows, setRows] = useState<Row[]>([]); const [cursor, setCursor] = useState<string | null>(null);
  const pages = useRef(1); const active = useRef<Operation | null>(null);
  const refresh = useRef<(() => Promise<void>) | null>(null);
  const [working, setWorking] = useState(false); const [error, setError] = useState('');
  const [edit, setEdit] = useState<{ id: string | null; persona: Persona } | null>(null);
  const [expanded, setExpanded] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    const observation = observeResource<Page>({client, points:['rintawa/character/changed/v1'], read: async signal => {
      const items: Row[] = []; let next: string | null = null;
      for (let i=0;i<pages.current;i++) { const page: Page = await domain.current.request('list-personas', next ? {after:next}: {},signal); items.push(...page.items); next=page.next; if (!next) break; }
      return {items,next};
    }, apply: page => {setRows(page.items); setCursor(page.next);}, failed: e => setError(String(e))});
    refresh.current = () => observation.refresh(); void observation.refresh().catch(() => {});
    return () => { controller.abort(); observation.close(); refresh.current=null; if(active.current) void domain.current.release(active.current).catch(()=>{}); };
  }, [client]);
  const run = async (fn:()=>Promise<void>) => {setWorking(true); setError('');try{await fn()}catch(e){setError(e instanceof Error?e.message:String(e))}finally{setWorking(false)}};
  const mutate = async (method:string, payload:Record<string,unknown>) => {
    const op=await domain.current.begin(method,payload);active.current=op;
    const result=await domain.current.wait(op);active.current=null;
    await refresh.current?.();
    if(result.status==='unknown') throw new Error('Outcome unknown. Read the persona list and revision before deciding whether to try again.');
    if(result.status==='failed') throw new Error(result.reason);
  };
  const disabled=busy||working;
  return <section className="persona-editor" aria-label="Player presets">
    {onSelection && <label>Player persona<select disabled={disabled} value={selection ?? ''} onChange={e=>onSelection(e.target.value||null)}><option value="">None (You)</option>{selection && !rows.some(r=>r.id===selection)&&<option value={selection}>Selected preset (not in loaded page)</option>}{rows.map(row=><option key={row.id} value={row.id}>{row.persona.name}</option>)}</select></label>}
    <div className="persona-actions"><Button disabled={disabled} onClick={()=>setExpanded(!expanded)}>{expanded?'Close player presets':'Manage player presets'}</Button>{cursor && <Button disabled={disabled} onClick={()=>void run(async()=>{pages.current++;await refresh.current?.()})}>More presets</Button>}</div>
    {error&&<Status error>{error}</Status>}
    {expanded&&<><div className="persona-actions"><Button disabled={disabled} onClick={()=>setEdit({id:null,persona:blank()})}>Create preset</Button><Button disabled={disabled} onClick={()=>void run(async()=>{await refresh.current?.()})}>Read presets</Button></div>
      {rows.map(row=><div className="persona-row" key={row.id}><strong>{row.persona.name}</strong><div className="persona-actions"><Button disabled={disabled} onClick={()=>void run(async()=>{setEdit(await domain.current.request<{id:string;persona:Persona}>('get-persona',{itemId:row.id}))})}>Edit</Button><Button disabled={disabled} onClick={()=>void run(async()=>{const [itemId]=await domain.current.allocate(1);await mutate('begin-duplicate-persona',{itemId,sourceItemId:row.id,expectedRevision:row.persona.revision})})}>Duplicate</Button><Button disabled={disabled} onClick={()=>void run(async()=>{await mutate('begin-delete-persona',{itemId:row.id,expectedRevision:row.persona.revision});if(selection===row.id)onSelection?.(null);if(edit?.id===row.id)setEdit(null)})}>Delete</Button></div></div>)}
      {edit&&<div className="persona-form">
        <label>Name<input disabled={disabled} required maxLength={512} value={edit.persona.name} onChange={e=>setEdit({...edit,persona:{...edit.persona,name:e.target.value}})} /></label>
        <label>Description<textarea disabled={disabled} maxLength={8192} value={edit.persona.description} onChange={e=>setEdit({...edit,persona:{...edit.persona,description:e.target.value}})} /></label>
        <div className="persona-actions"><button type="button" className="rtw-button primary" disabled={disabled||!edit.persona.name.trim()} onClick={()=>void run(async()=>{const {persona,id}=edit;const itemId=id??(await domain.current.allocate(1))[0];const {name,description,portrait}=persona;await mutate(id?'begin-update-persona':'begin-create-persona',{itemId,name,description,portrait,...(id?{expectedRevision:persona.revision}:{})});setEdit(null)})}>Save</button><Button disabled={disabled} onClick={()=>setEdit(null)}>Cancel</Button></div>
      </div>}
    </>}
  </section>;
}
