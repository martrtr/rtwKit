import { useEffect, useRef, useState } from 'react';
import { BridgeError, type RootBridge } from '@rintawa/bridge';
import { Button, Dialog, Input, Status } from '@rintawa/ui';
import './Secrets.css';
type Entry = {id:string;label:string;revision:number};
type Page<T> = {items:T[];nextOffset:number;total:number;backend?:string};
type Picker = {packageName:string;slot:string;target:{scopeId:string;instanceId:string;componentId:string;artifact:string;activation:string}};
type Grant = {scope_id:string;instance_id:string;component_id:string;path:string;entry_id?:string};
export function Secrets({bridge,reference,close}:{bridge:RootBridge;reference?:string;close:()=>void}) {
  const [entries,setEntries]=useState<Entry[]>([]),[consumers,setConsumers]=useState<Grant[]>([]),[backend,setBackend]=useState(''),[picker,setPicker]=useState<Picker|null>(null);
  const [selected,setSelected]=useState(''),[label,setLabel]=useState(''),[value,setValue]=useState(''),[reveal,setReveal]=useState(false);
  const [busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState(''),[uncertain,setUncertain]=useState(false);
  const [edit,setEdit]=useState<'create'|'rename'|'replace'|null>(null),[confirmation,setConfirmation]=useState<'replace'|'delete'|Grant|null>(null);
  const bound=useRef(false);
  const active=useRef(true),stop=useRef(new AbortController());
  const operation=(request:Record<string,unknown>)=>bridge.secretOperation(request,stop.current.signal);
  const load=async()=>{
    const all:Entry[]=[];let offset=0;
    do {const page=await operation({operation:'list',offset}) as Page<Entry>;all.push(...page.items);offset=page.nextOffset;setBackend(page.backend??'');if(offset>=page.total)break;}while(active.current);
    if(!active.current)return;setEntries(all);
    if(reference){setPicker(await operation({operation:'inspect-prompt',prompt:reference}) as Picker);}
    else {const all:Grant[]=[];let offset=0;do {const page=await operation({operation:'consumers',offset}) as Page<Grant>;all.push(...page.items);offset=page.nextOffset;if(offset>=page.total)break;}while(active.current);if(active.current)setConsumers(all);}
  };
  useEffect(()=>{active.current=true;void load().catch(()=>{if(active.current)setError('Secrets are unavailable. Check permission, backend and the requesting extension.');});return()=>{active.current=false;stop.current.abort();if(reference&&!bound.current)void bridge.client.request('secret-prompt.dismiss',{reference}).catch(()=>{});};},[bridge,reference]);
  const dismiss=()=>{setValue('');setReveal(false);stop.current.abort();close();};
  const run=async(request:Record<string,unknown>,done?:()=>void,finish=false)=>{
    setBusy(true);setError('');setNotice('');
    try{await operation(request);if(active.current){done?.();if(finish)return;setEdit(null);setConfirmation(null);setNotice('Saved.');await load();}}
    catch(failure){if(active.current){const message=failure instanceof Error?failure.message:'Secret operation failed';setError(message==='message-too-large'?'Use native CLI stdin to save a value of this size.':message);setUncertain(!(failure instanceof BridgeError&&failure.outcome==='not-started'));}}
    finally{if(active.current)setBusy(false);}
  };
  const current=entries.find(e=>e.id===selected);
  const submit=()=>{
    const entered=value;setValue('');setReveal(false);
    if(edit==='create')void run({operation:'create',label,value:entered});
    if(edit==='rename'&&current)void run({operation:'rename',id:current.id,revision:current.revision,label});
    if(edit==='replace'&&current)void run({operation:'replace',id:current.id,revision:current.revision,value:entered});
  };
  return <Dialog open title={reference?'Select credential':'Secrets'} onClose={dismiss}>
    <div className="secret-dialog">
      {reference ? picker ? <><p><strong>{picker.packageName}</strong> requests a saved credential.</p><p>{picker.target.scopeId} · {picker.target.componentId}</p><details><summary>Target details</summary><code>{picker.target.instanceId}<br/>{picker.target.artifact}<br/>{picker.slot}</code></details></> : <Status>Checking requester…</Status> : <p>Manage shared credentials and their connections.</p>}
      {backend&&<p>Storage: {backend==='system'?'System credentials':'Local file'}</p>}
      <label htmlFor="secret-choice">Saved credential</label><select id="secret-choice" disabled={busy||!!edit} value={selected} onChange={e=>{setSelected(e.target.value);setConfirmation(null);}}><option value="">Choose a credential</option>{entries.map(e=><option key={e.id} value={e.id}>{e.label}</option>)}</select>
      {!edit&&<div><Button disabled={busy||uncertain||!backend||!!reference&&!picker} onClick={()=>{setLabel('');setValue('');setEdit('create');}}>Create credential</Button>{!reference&&current&&<><Button disabled={busy||uncertain} onClick={()=>{setLabel(current.label);setEdit('rename');}}>Rename</Button><Button disabled={busy||uncertain} onClick={()=>setConfirmation('replace')}>Replace value…</Button><Button disabled={busy||uncertain} onClick={()=>setConfirmation('delete')}>Delete credential…</Button></>}</div>}
      {edit&&<form onSubmit={e=>{e.preventDefault();submit();}}>
        <h3>{edit==='create'?'Create credential':edit==='rename'?'Rename credential':'Replace credential value'}</h3>
        {edit!=='replace'&&<><label htmlFor="secret-label">Label</label><Input id="secret-label" value={label} maxLength={128} disabled={busy} onChange={e=>setLabel(e.target.value)}/></>}
        {edit!=='rename'&&<><label htmlFor="secret-value">Value</label>{!reveal&&!CSS.supports('-webkit-text-security','disc')?<Input type="password" id="secret-value"  value={value} autoComplete="off" autoCorrect="off" spellCheck={false} disabled={busy} onChange={e=>setValue(e.target.value)} onPaste={e=>{e.preventDefault();const input=e.currentTarget;const text=e.clipboardData.getData('text/plain');setValue(previous=>previous.slice(0,input.selectionStart??0)+text+previous.slice(input.selectionEnd??0));}}/>:<textarea id="secret-value" className={reveal?'secret-revealed':'secret-masked'} value={value} autoComplete="off" autoCorrect="off" spellCheck={false} disabled={busy} onChange={e=>setValue(e.target.value)} onPaste={e=>{e.preventDefault();const input=e.currentTarget;const text=e.clipboardData.getData('text/plain');setValue(previous=>previous.slice(0,input.selectionStart??0)+text+previous.slice(input.selectionEnd??0));}}/>}<label><input type="checkbox" checked={reveal} onChange={e=>setReveal(e.target.checked)}/> Reveal value</label><p>{new TextEncoder().encode(value).length} bytes</p></>}
        <Button type="button" disabled={busy} onClick={()=>{setEdit(null);setValue('');setReveal(false);}}>Cancel entry</Button><Button type="submit" disabled={busy||uncertain||(edit!=='replace'&&!label)||(edit!=='rename'&&!value)}>Save credential</Button>
      </form>}
      {confirmation&&<div role="alert"><p>{confirmation==='replace'?'Replacing this value affects every connected consumer.':confirmation==='delete'?'Delete this credential? Connected consumers will lose access.':'Disconnect exactly this consumer from its credential?'}</p><Button disabled={busy} onClick={()=>setConfirmation(null)}>Keep unchanged</Button><Button disabled={busy||uncertain} onClick={()=>{if(confirmation==='replace'){setEdit('replace');setValue('');setConfirmation(null);}else if(confirmation==='delete'&&current)void run({operation:'delete',id:current.id,revision:current.revision},()=>setSelected(''));else if(typeof confirmation==='object')void run({operation:'disconnect',grant:confirmation});}}>Confirm</Button></div>}
      {!reference&&<div><h3>Connected consumers</h3>{consumers.filter(g=>!current||g.entry_id===current.id).map(g=><div key={`${g.scope_id}/${g.instance_id}/${g.component_id}/${g.path}`}><p>{g.entry_id?(entries.find(e=>e.id===g.entry_id)?.label??"Missing credential"):"Legacy direct credential"}<br/>{g.scope_id} · {g.component_id}<br/><code>{g.path}</code></p><Button disabled={busy||uncertain} onClick={()=>setConfirmation(g)}>Disconnect…</Button></div>)}</div>}
      {notice&&<Status>{notice}</Status>}{error&&<Status error>{error}{uncertain&&' Check current state before another change.'}</Status>}
      <footer><Button onClick={dismiss}>Close</Button>{uncertain&&<Button disabled={busy} onClick={()=>{setBusy(true);void load().then(()=>{setUncertain(false);setError('');setEdit(null);setConfirmation(null);setValue('');}).catch(()=>setError('Current state unavailable.')).finally(()=>setBusy(false));}}>Check current state</Button>}{reference&&<Button className="primary" disabled={busy||uncertain||!picker||!current||!!edit} onClick={()=>void run({operation:'bind-prompt',prompt:reference,id:selected},()=>{bound.current=true;dismiss();},true)}>Use credential</Button>}</footer>
    </div>
  </Dialog>;
}
