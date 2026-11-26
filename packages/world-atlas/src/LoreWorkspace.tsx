import { useEffect, useRef, useState, type RefObject } from 'react';
import { BridgeError, type Client } from '@rintawa/bridge';
import { DomainClient, DomainError, type Operation } from '@rintawa/domain';
import { Button, Status } from '@rintawa/ui';

type Lore = {revision:string;title:string;body:string;keywords:string[];exclusions:string[];alwaysOn:boolean;enabled:boolean;priority:number;conversationId:string|null};
type Item = Lore & {id:string};
type Summary = Omit<Item,'body'>;
type Page<T> = {items:T[];next:string|null};
type Conversation = {id:string;conversation:{title:string}};
type Selection = {entries:{id:string;revision:string;title:string;body:string}[];truncated:boolean;omitted:number};
type Uncertain = {id:string;checked:boolean};
const fresh=():Lore=>({revision:'1',title:'',body:'',keywords:[],exclusions:[],alwaysOn:false,enabled:false,priority:0,conversationId:null});
const bytes=(s:string)=>new TextEncoder().encode(s).length;
const describe=(e:unknown)=>e instanceof Error?e.message:'Request unavailable';
export function LoreWorkspace({client,pending,onBusy}:{client:Client;pending:RefObject<Promise<void>|null>;onBusy:(busy:boolean)=>void}){
  const domain=useRef(new DomainClient(client,'rintawa/world-atlas/v1'));
  const chats=useRef(new DomainClient(client,'rintawa/chat/v1'));
  const alive=useRef(true),working=useRef(false),stop=useRef<AbortController|null>(null);
  const [items,setItems]=useState<Summary[]>([]),[more,setMore]=useState<string|null>(null);
  const [conversations,setConversations]=useState<Conversation[]>([]),[moreChats,setMoreChats]=useState<string|null>(null);
  const [id,setId]=useState<string|null>(null),[draft,setDraft]=useState<Lore|null>(null);
  const [keywords,setKeywords]=useState(''),[exclusions,setExclusions]=useState('');
  const [busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('');
  const [unknown,setUnknown]=useState<Uncertain|null>(null);
  const [previewText,setPreviewText]=useState(''),[previewChat,setPreviewChat]=useState(''),[preview,setPreview]=useState<Selection|null>(null);
  const run=(action:()=>Promise<void>)=>{
    if(working.current||!alive.current)return;
    working.current=true;setBusy(true);onBusy(true);setError('');stop.current=new AbortController();
    const task=(async()=>{try{await action();}catch(e){if(alive.current)setError(describe(e));}finally{working.current=false;stop.current=null;if(alive.current){setBusy(false);onBusy(false);}}})();
    pending.current=task;
    void task.finally(()=>{if(pending.current===task)pending.current=null;});
  };
  async function list(after?:string){
    const page=await domain.current.request<Page<Summary>>('list-lore',after?{after}:{},stop.current?.signal);
    if(!alive.current)return;
    setItems(old=>after?[...new Map([...old,...page.items].map(i=>[i.id,i])).values()]:page.items);setMore(page.next);
  }
  async function listChats(after?:string){
    const page=await chats.current.request<Page<Conversation>>('list',after?{after}:{},stop.current?.signal);
    if(!alive.current)return;setConversations(old=>after?[...old,...page.items]:page.items);setMoreChats(page.next);
  }
  useEffect(()=>{alive.current=true;run(async()=>{await list();});return()=>{alive.current=false;stop.current?.abort();};},[]);
  function show(item:Item){setId(item.id);setDraft(item);setKeywords(item.keywords.join('\n'));setExclusions(item.exclusions.join('\n'));setPreview(null);setNotice('');}
  async function save(){
    if(!draft)return;
    const captured={...draft,keywords:terms(keywords),exclusions:terms(exclusions)};
    // DTO-only id is never part of the strict persistent record.
    const record:Lore={revision:captured.revision,title:captured.title,body:captured.body,keywords:captured.keywords,exclusions:captured.exclusions,alwaysOn:captured.alwaysOn,enabled:captured.enabled,priority:captured.priority,conversationId:captured.conversationId};
    let itemId=id,operation:Operation|undefined,begun=false,known=false;
    try{
      if(!itemId)[itemId]=await domain.current.allocate(1);
      if(!alive.current)return;
      begun=true;
      operation=await domain.current.begin(id?'begin-update-lore':'begin-create-lore',{itemId,record,...(id?{expectedRevision:draft.revision}:{})});
      if(!alive.current)return;
      const result=await domain.current.wait(operation,stop.current?.signal);
      if(!alive.current)return;
      if(result.status==='unknown'){setUnknown({id:itemId,checked:false});setNotice('Save not confirmed. Check the saved entry before another change.');return;}
      known=true;if(result.status==='failed')throw new Error(result.reason);
      const saved=await domain.current.request<Item>('get-lore',{itemId});if(!alive.current)return;
      show(saved);setNotice('Saved.');await list();
    }catch(e){
      const notStarted=(e instanceof DomainError||e instanceof BridgeError)&&e.outcome==='not-started';
      if(alive.current&&begun&&!known&&(operation||!notStarted)&&itemId)setUnknown({id:itemId,checked:false});
      throw e;
    }finally{if(operation)await domain.current.release(operation).catch(()=>{});}
  }
  const terms=(s:string)=>s.split('\n').map(s=>s.trim()).filter(Boolean);
  const validTerms=(s:string)=>terms(s).length<=8&&terms(s).every(s=>bytes(s)<=128);
  const valid=!!draft&&!!draft.title.trim()&&bytes(draft.title)<=256&&!!draft.body.trim()&&bytes(draft.body)<=4096&&validTerms(keywords)&&validTerms(exclusions)&&Number.isInteger(draft.priority)&&draft.priority>=-100&&draft.priority<=100;
  const blocked=busy||!!unknown;
  function patch(value:Partial<Lore>){if(draft)setDraft({...draft,...value});setPreview(null);}
  return <div className="atlas-lore">
    <p>Author notes about this world. Enable a note to let Narrator include it when its conditions match. This does not give every character knowledge of it.</p><small>This first version supports 64 notes per world, including disabled notes.</small>
    {error&&<Status error>{error}</Status>}{notice&&<Status>{notice}</Status>}{busy&&<Status>Working…</Status>}
    {unknown&&<section><h2>Check an unconfirmed save</h2><p>Your draft is kept. Checking does not repeat the save.</p><Button disabled={busy} onClick={()=>run(async()=>{
      try{const saved=await domain.current.request<Item>('get-lore',{itemId:unknown.id});if(!alive.current)return;setId(saved.id);setDraft(old=>old?{...old,revision:saved.revision}:old);setNotice(`Saved text: ${saved.body}`);setUnknown({...unknown,checked:true});}
      catch(e){if(e instanceof DomainError&&['not-found','item-not-found'].includes(e.code)){if(alive.current){setUnknown({...unknown,checked:true});setNotice('Entry not found. This alone does not prove rollback.');}}else throw e;}
    })}>Check saved entry</Button><Button disabled={busy||!unknown.checked} onClick={()=>{setUnknown(null);setNotice('Saved state reviewed. Any next save is a new explicit action.');}}>I have reviewed it</Button></section>}
    <div className="world-atlas-body"><nav aria-label="Lore entries"><section><h2>Lorebook <Button disabled={blocked} onClick={()=>{setId(null);setDraft(fresh());setKeywords('');setExclusions('');setNotice('');setPreview(null);}}>New note</Button></h2>
      {items.map(item=><button className={`row${id===item.id?' selected':''}`} key={item.id} disabled={blocked} onClick={()=>run(async()=>{const value=await domain.current.request<Item>('get-lore',{itemId:item.id});if(alive.current)show(value);})}><strong>{item.title}</strong><small>{item.enabled?'Enabled for narration':'Disabled'} · {item.alwaysOn?'Always relevant':'Keyword match'}</small></button>)}
      {!items.length&&<small>No notes loaded.</small>}{more&&<Button disabled={blocked} onClick={()=>run(()=>list(more))}>More notes</Button>}
      <Button disabled={blocked} onClick={()=>run(()=>list())}>Reload notes</Button>
    </section></nav>
    <section className="editor" aria-label="Lore editor">{draft?<><h2>{id?'Edit note':'New note'}</h2>
      <label>Title<input aria-label="Lore title" value={draft.title} disabled={blocked} onChange={e=>patch({title:e.target.value})}/></label>
      <label>Note<textarea aria-label="Lore body" value={draft.body} disabled={blocked} onChange={e=>patch({body:e.target.value})}/></label><small>{bytes(draft.body)} / 4096 UTF-8 bytes</small>
      <label><input type="checkbox" checked={draft.enabled} disabled={blocked} onChange={e=>patch({enabled:e.target.checked})}/> Use this note in narration</label>
      <label><input type="checkbox" checked={draft.alwaysOn} disabled={blocked} onChange={e=>patch({alwaysOn:e.target.checked})}/> Always relevant within its conversation scope</label>
      <label>Include when text contains (one phrase per line)<textarea aria-label="Lore keywords" value={keywords} disabled={blocked} onChange={e=>{setKeywords(e.target.value);setPreview(null);}}/></label>
      <label>Exclude when text contains (overrides always relevant)<textarea aria-label="Lore exclusions" value={exclusions} disabled={blocked} onChange={e=>{setExclusions(e.target.value);setPreview(null);}}/></label>
      <small>Up to 8 phrases per field, 128 UTF-8 bytes each. Literal, case-insensitive substring matching. No keywords means no match unless always relevant.</small>
      <label>Priority<input aria-label="Lore priority" type="number" min={-100} max={100} step={1} value={Number.isNaN(draft.priority)?'':draft.priority} disabled={blocked} onChange={e=>patch({priority:e.target.value===''?NaN:Number(e.target.value)})}/></label>
      <label>Conversation<select aria-label="Lore conversation" value={draft.conversationId??''} disabled={blocked} onFocus={()=>{if(!conversations.length&&!working.current)run(()=>listChats());}} onChange={e=>patch({conversationId:e.target.value||null})}><option value="">All conversations</option>{draft.conversationId&&!conversations.some(c=>c.id===draft.conversationId)&&<option value={draft.conversationId}>Saved conversation</option>}{conversations.map(c=><option key={c.id} value={c.id}>{c.conversation.title}</option>)}</select></label>
      {moreChats&&<Button disabled={blocked} onClick={()=>run(()=>listChats(moreChats))}>More conversations</Button>}
      <div className="actions"><Button className="primary" disabled={blocked||!valid} onClick={()=>run(save)}>Save note</Button><Button disabled={blocked} onClick={()=>setDraft(null)}>Close</Button></div>
      {error&&id&&<Button disabled={blocked} onClick={()=>run(async()=>{const saved=await domain.current.request<Item>('get-lore',{itemId:id});if(alive.current){setDraft(old=>old?{...old,revision:saved.revision}:old);setNotice(`Saved text: ${saved.body}. Your draft is unchanged.`);}})}>Check saved version, keep draft</Button>}
    </>:<p>Select a note or create one. Notes start disabled; enable them when ready to use in replies.</p>}</section></div>
    <details className="lore-preview"><summary>Preview relevant saved notes</summary><div className="editor"><p>Uses saved notes and the same selector as Narrator. Unsaved changes do not affect this preview. No AI call.</p><label>Conversation<select aria-label="Preview conversation" value={previewChat} disabled={blocked} onFocus={()=>{if(!conversations.length&&!working.current)run(()=>listChats());}} onChange={e=>{setPreviewChat(e.target.value);setPreview(null);}}><option value="">Choose conversation</option>{conversations.map(c=><option key={c.id} value={c.id}>{c.conversation.title}</option>)}</select></label><label>Example context<textarea aria-label="Lore preview text" value={previewText} disabled={blocked} onChange={e=>{setPreviewText(e.target.value);setPreview(null);}}/></label><Button disabled={blocked||!previewChat||bytes(previewText)>16384} onClick={()=>run(async()=>{const result=await domain.current.request<Selection>('select-lore',{conversationId:previewChat,text:previewText});if(alive.current)setPreview(result);})}>Preview selection</Button>
      {preview&&<section aria-label="Lore selection">{preview.entries.length===0&&<p>No notes match.</p>}{preview.entries.map(e=><article key={e.id}><h3>{e.title}</h3><p className="lore-body">{e.body}</p></article>)}{preview.truncated&&<small>{preview.omitted} matching notes omitted by the context budget.</small>}</section>}
    </div></details>
  </div>;
}
