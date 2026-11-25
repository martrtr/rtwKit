import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, BridgeError, type Client } from '@rintawa/bridge';
import { DomainClient, DomainError } from '@rintawa/domain';
import { Button, Dialog, Icon, Status } from '@rintawa/ui';
import { uploadPackage } from '@rintawa/ui/packages';
import '@rintawa/ui/style.css';
import './style.css';

type Artifact = {digest:string;content:string|null;name:string;version:string|null;subject:string|null;error?:string|null};
type Selection = {subject:string;name:string;version:string|null;digest:string;enabled:boolean;instanceId:string;scopeId:string};
type World = {worldId:string;title:string;active:boolean;pendingActive:boolean|null};
type Page = {items:Artifact[];next:string|null};
type Row = {key:string;artifact:Artifact;versions:Artifact[];selection?:Selection};
type Tab = 'installed'|'library';
type Review = {artifact:Artifact;scope:string;scopeTitle:string;components:{componentId:string;requested:string[]}[]};
const permissions=['background-task','composition-read','composition-write','runtime-policy-read','world-session-read','world-session-write'];
const isExtension=(item:Artifact)=>item.content==='rintawa.extension@1';
const kind=(item:Artifact)=>isExtension(item)?'Extension':item.content==='rintawa.world-template@1'?'World template':item.error?'Unreadable':'Other';
const fingerprint=(digest:string)=>digest.startsWith('sha256:')?digest.slice(7,19):'Unavailable';
const versionLabel=(item:Artifact)=>(item.version??'Unversioned')+' · '+fingerprint(item.digest);
function message(failure:unknown){
 const value=failure instanceof Error?failure.message:'Request unavailable';
 if(/invalid zip|zip archive|zip container/i.test(value))return 'This file is not a valid RTW package. It may be damaged or incomplete.';
 if(/permission.?denied/i.test(value))return 'Packages needs permission to access this configuration.';
 if(value==='unavailable')return 'The connection is unavailable. Refresh to try again.';
 return value;
}
function fromSelection(selection:Selection):Artifact{
 return {digest:selection.digest,content:'rintawa.extension@1',name:selection.name,version:selection.version,subject:selection.subject};
}
function Packages(){
 const bridge=useRef<Client|null>(null),domain=useRef<DomainClient|null>(null),mounted=useRef(true),uploadAbort=useRef<AbortController|null>(null),fileInput=useRef<HTMLInputElement>(null);
 const rowElements=useRef(new Map<string,HTMLTableRowElement>());
 const [worlds,setWorlds]=useState<World[]>([]),[scope,setScope]=useState('host'),[artifacts,setArtifacts]=useState<Artifact[]>([]),[next,setNext]=useState<string|null>(null),[selections,setSelections]=useState<Selection[]>([]);
 const [busy,setBusy]=useState(true),[ready,setReady]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState(''),[unknown,setUnknown]=useState(false),[review,setReview]=useState<Review|null>(null);
 const [tab,setTab]=useState<Tab>('installed'),[search,setSearch]=useState(''),[filter,setFilter]=useState('all'),[selectedKey,setSelectedKey]=useState<string|null>(null),[chosenVersion,setChosenVersion]=useState<string|null>(null),[dragging,setDragging]=useState(false);
 const selectedWorld=worlds.find(world=>'world:'+world.worldId===scope);
 const scopeTitle=scope==='host'?'Home':scope==='default-world'?'New World defaults':selectedWorld?.title??'World';
 const locked=!!selectedWorld&&(selectedWorld.active||selectedWorld.pendingActive!==null);
 const changesBlocked=busy||locked||unknown||!ready;
 const scopeNote=scope==='host'?'Changes apply after restarting Rintawa.':scope==='default-world'?'Used by new Worlds. Existing Worlds keep their setup.':'Changes apply the next time this World starts.';
 const load=async(target:string)=>{
  const api=domain.current!;
  const description=await api.request<{worlds:World[]}>('describe');
  const installed=await api.request<Selection[]>('selections',{scopeId:target});
  if(!mounted.current)return;
  setWorlds(description.worlds);setSelections(installed);setReady(true);
  try{const page=await api.request<Page>('inventory');if(mounted.current){setArtifacts(page.items);setNext(page.next);}}
  catch(failure){if(mounted.current)setError('The library could not be refreshed. Import is still available. '+message(failure));}
 };
 useEffect(()=>{
  mounted.current=true;
  void connectPanel().then(async client=>{
   if(!mounted.current){client.dispose();return;}
   bridge.current=client;domain.current=new DomainClient(client,'rintawa/packages/v1');
   try{await load('host');}catch(failure){if(mounted.current)setError(message(failure));}finally{if(mounted.current)setBusy(false);}
  }).catch(failure=>{if(mounted.current){setError(message(failure));setBusy(false);}});
  return()=>{mounted.current=false;uploadAbort.current?.abort();bridge.current?.dispose();};
 },[]);
 const read=async(action:()=>Promise<void>)=>{
  setBusy(true);setError('');
  try{await action();}catch(failure){if(mounted.current)setError(message(failure));}finally{if(mounted.current)setBusy(false);}
 };
 const refresh=()=>read(async()=>{await load(scope);if(mounted.current){setUnknown(false);setNotice('Configuration refreshed.');}});
 const chooseScope=(target:string)=>{
  if(busy||target===scope)return;
  setScope(target);setSelections([]);setSelectedKey(null);setChosenVersion(null);setNotice('');setSearch('');
  void read(()=>load(target));
 };
 const changeTab=(value:Tab)=>{setTab(value);setSelectedKey(null);setChosenVersion(null);setSearch('');setFilter('all');};
 const groups=useMemo(()=>{
  const result=new Map<string,Artifact[]>();
  for(const artifact of artifacts){const key=isExtension(artifact)&&artifact.subject?'package:'+artifact.subject:artifact.digest;const items=result.get(key)??[];items.push(artifact);result.set(key,items);}
  return [...result].map(([key,versions])=>{
   versions.sort((a,b)=>(b.version??'').localeCompare(a.version??'',undefined,{numeric:true})||a.digest.localeCompare(b.digest));
   const selected=selections.find(item=>versions.some(version=>version.digest===item.digest));
   return {key,versions,artifact:versions.find(item=>item.digest===selected?.digest)??versions[0],selection:selected};
  });
 },[artifacts,selections]);
 const rows=useMemo(()=>{
  const items:Row[]=tab==='library'?groups:selections.map(selection=>{
   const artifact=artifacts.find(item=>item.digest===selection.digest)??fromSelection(selection);
   const versions=[artifact,...artifacts.filter(item=>item.subject===selection.subject&&item.digest!==artifact.digest)];
   return {key:selection.subject,artifact,versions,selection};
  });
  const query=search.trim().toLocaleLowerCase();
  return items.filter(row=>(filter==='all'||kind(row.artifact)===filter)&&(!query||[row.artifact.name,row.artifact.subject??'',...row.versions.flatMap(item=>[item.version??'',item.digest])].some(value=>value.toLocaleLowerCase().includes(query)))).sort((a,b)=>a.artifact.name.localeCompare(b.artifact.name));
 },[tab,groups,selections,artifacts,search,filter]);
 const selected=rows.find(row=>row.key===selectedKey)??rows[0]??null;
 const candidate=selected?.versions.find(item=>item.digest===chosenVersion)??selected?.artifact??null;
 const installedCandidate=candidate?selections.find(item=>item.digest===candidate.digest):undefined;
 const installedPackage=candidate?.subject?selections.find(item=>item.subject===candidate.subject):selected?.selection;
 const selectRow=(key:string)=>{if(selected?.key===key)return;setSelectedKey(key);setChosenVersion(null);};
 const moveSelection=(event:KeyboardEvent<HTMLTableRowElement>,index:number)=>{
  if(busy||event.target!==event.currentTarget||!['ArrowDown','ArrowUp','Home','End'].includes(event.key))return;
  event.preventDefault();const target=rows[event.key==='Home'?0:event.key==='End'?rows.length-1:Math.max(0,Math.min(rows.length-1,index+(event.key==='ArrowUp'?-1:1)))];
  if(target){selectRow(target.key);rowElements.current.get(target.key)?.focus();}
 };
 const mutate=async(method:string,args:Record<string,unknown>,label:string,installedSubject?:string)=>{
  const targetScope=String(args.scopeId);setBusy(true);setError('');setNotice('');let saved=false;
  try{
   await domain.current!.request(method,args);saved=true;
   if(!mounted.current)return;
   setReview(null);setNotice(label);await load(targetScope);
   if(installedSubject){setTab('installed');setSelectedKey(installedSubject);setChosenVersion(null);setSearch('');}
  }catch(failure){if(mounted.current){setError(message(failure));if(!saved){setUnknown(true);setNotice('Refresh before another change: the result was not confirmed.');}else setNotice(label+' Refresh to reload the list.');}}
  finally{if(mounted.current)setBusy(false);}
 };
 const inspect=async(artifact:Artifact)=>{
  const target=scope,title=scopeTitle;
  await read(async()=>{
   const info=await domain.current!.request<{name:string;version:string;subject:string;components:Review['components']}>('inspect',{digest:artifact.digest});
   if(mounted.current)setReview({artifact:{...artifact,name:info.name,version:info.version,subject:info.subject},scope:target,scopeTitle:title,components:info.components});
  });
 };
 const importFiles=async(files:File[])=>{
  if(!files.length||busy||!ready)return;
  const controller=new AbortController();uploadAbort.current=controller;setBusy(true);setError('');setNotice('');let confirmed:string|null=null;
  const adapter={request:async(method:string,args:Record<string,unknown>={})=>{
   if(!method.startsWith('packages.'))throw Error('Invalid transfer request');
   try{return await domain.current!.request(method.slice(9),args);}catch(failure){if(failure instanceof DomainError)throw new BridgeError(failure.code,failure.outcome);throw failure;}
  }};
  try{
   for(const file of files){
    if(!mounted.current||controller.signal.aborted)return;
    await uploadPackage(adapter,file,controller.signal,percent=>{if(mounted.current)setNotice(`Importing ${file.name}… ${percent}%`);});confirmed=file.name;
   }
   await load(scope);
   if(mounted.current){changeTab('library');setNotice(files.length===1?`Imported ${files[0].name}. Select it to install.`:`Imported ${files.length} packages. Select a package to install.`);}
  }catch(failure){if(mounted.current){setError(message(failure));setNotice(confirmed?`Imported ${confirmed}. Refresh to check the remaining files.`:'Import was not confirmed. Refresh before retrying.');}}
  finally{if(uploadAbort.current===controller)uploadAbort.current=null;if(mounted.current)setBusy(false);}
 };
 const allow=async()=>{try{await bridge.current!.request('ui.request-permissions',{componentId:'backend',runtime:permissions,httpOrigins:[],secretRead:[]});}catch(failure){setError(message(failure));}};
 const createWorld=async(artifact:Artifact)=>{try{await bridge.current!.request('ui.create-world',{template:artifact.digest});}catch(failure){setError(message(failure));}};
 const browse=()=>{changeTab('library');setFilter('Extension');};
 const status=unknown?'Refresh required':busy?'Working…':notice||`${selections.length} installed · ${artifacts.length} local archives${next?' loaded':''}`;
 return <main className={'packages'+(dragging?' is-dragging':'')} onDragOver={event=>{if(event.dataTransfer.types.includes('Files')){event.preventDefault();setDragging(true);}}} onDragLeave={event=>{if(!(event.relatedTarget instanceof Node)||!event.currentTarget.contains(event.relatedTarget))setDragging(false);}} onDrop={event=>{event.preventDefault();setDragging(false);void importFiles([...event.dataTransfer.files]);}}>
  <header className="packages-toolbar"><h1><Icon name="grid"/>Packages</h1><div className="toolbar-actions"><Button className="primary" disabled={busy||!ready} onClick={()=>fileInput.current?.click()}><Icon name="plus" size={16}/>Import from file</Button><Button title="Refresh" aria-label="Refresh" disabled={busy||!domain.current} onClick={()=>void refresh()}><Icon name="refresh" size={16}/></Button><input ref={fileInput} type="file" hidden multiple accept=".rtw" aria-label="Import RTW" disabled={busy||!ready} onChange={event=>{const files=[...(event.currentTarget.files??[])];event.currentTarget.value='';void importFiles(files);}}/></div></header>
  {error&&<div className="packages-error" role="alert"><span>{error}</span><button aria-label="Dismiss error" onClick={()=>setError('')}><Icon name="close" size={15}/></button></div>}
  <div className="packages-workspace">
   <aside className="scope-panel" aria-label="Installation targets"><h2>Install in</h2><button data-scope="host" className={scope==='host'?'active':''} aria-current={scope==='host'?'page':undefined} disabled={busy} onClick={()=>chooseScope('host')}><Icon name="grid" size={17}/><span>Home<small>Host extensions</small></span></button><button data-scope="default-world" className={scope==='default-world'?'active':''} aria-current={scope==='default-world'?'page':undefined} disabled={busy} onClick={()=>chooseScope('default-world')}><Icon name="plus" size={17}/><span>New World defaults<small>Future Worlds only</small></span></button><h2>Worlds</h2><div className="scope-worlds">{worlds.map(world=><button key={world.worldId} data-scope={'world:'+world.worldId} title={world.title} className={scope==='world:'+world.worldId?'active':''} aria-current={scope==='world:'+world.worldId?'page':undefined} disabled={busy} onClick={()=>chooseScope('world:'+world.worldId)}><Icon name="world" size={17}/><span>{world.title}<small>{world.active?'Running':'Stopped'}</small></span></button>)}{!worlds.length&&<p>No Worlds yet</p>}</div></aside>
   <section className="package-browser"><header className="browser-heading"><div><h2>{scopeTitle}</h2><p>{scopeNote}</p></div>{tab==='installed'&&<Button disabled={busy||!ready} onClick={browse}><Icon name="plus" size={15}/>Add extensions</Button>}</header>
    <select className="mobile-scope" aria-label="Package scope" value={scope} disabled={busy} onChange={event=>chooseScope(event.target.value)}><option value="host">Home</option><option value="default-world">New World defaults</option>{worlds.map(world=><option key={world.worldId} value={'world:'+world.worldId}>{world.title}</option>)}</select>
    <div className="package-tabs" role="tablist" aria-label="Package lists"><button role="tab" aria-selected={tab==='installed'} onClick={()=>changeTab('installed')}>Installed <span>{selections.length}</span></button><button role="tab" aria-selected={tab==='library'} onClick={()=>changeTab('library')}>Local library <span>{groups.length}{next?'＋':''}</span></button></div>
    <div className="package-filters"><label className="package-search"><Icon name="search" size={16}/><input aria-label="Search packages" placeholder="Search packages…" value={search} onChange={event=>setSearch(event.target.value)}/>{search&&<button aria-label="Clear search" onClick={()=>setSearch('')}><Icon name="close" size={14}/></button>}</label>{tab==='library'&&<select aria-label="Package type" value={filter} onChange={event=>setFilter(event.target.value)}><option value="all">All types</option><option value="Extension">Extensions</option><option value="World template">World templates</option><option value="Other">Other</option><option value="Unreadable">Unreadable</option></select>}</div>
    {locked&&<div className="scope-lock"><Icon name="power" size={14}/>Stop this World before changing its extensions.</div>}
    <div className="package-table-scroll"><table className="package-table" role="grid" aria-label={tab==='installed'?'Installed extensions':'Local package library'}><thead><tr>{tab==='installed'&&<th className="enable-column"><span className="sr-only">Enabled</span></th>}<th>Name</th><th>Version</th><th>{tab==='installed'?'Status':'Type'}</th></tr></thead><tbody>{rows.map((row,index)=><tr key={row.key} ref={element=>{if(element)rowElements.current.set(row.key,element);else rowElements.current.delete(row.key);}} data-package-key={row.key} data-archive-digests={row.versions.map(item=>item.digest).join(' ')} aria-selected={selected?.key===row.key} tabIndex={selected?.key===row.key?0:-1} className={selected?.key===row.key?'selected':''} onClick={()=>{if(!busy)selectRow(row.key);}} onKeyDown={event=>moveSelection(event,index)} onDoubleClick={()=>{if(!changesBlocked&&isExtension(row.artifact))void inspect(row.artifact);}}>
     {tab==='installed'&&<td className="enable-column"><input type="checkbox" aria-label={`Enable ${row.artifact.name}`} checked={row.selection!.enabled} disabled={changesBlocked} onClick={event=>event.stopPropagation()} onChange={event=>{selectRow(row.key);void mutate('set-enabled',{scopeId:scope,subject:row.selection!.subject,enabled:event.target.checked},`${row.artifact.name} ${event.target.checked?'enabled':'disabled'}.`);}}/></td>}
     <td><span className="package-name"><Icon name={kind(row.artifact)==='World template'?'world':'grid'} size={17}/><span>{row.artifact.name}</span>{row.artifact.error&&<span className="row-error" title={row.artifact.error}>!</span>}</span></td><td className="package-version">{row.artifact.version??'—'}{row.versions.length>1&&<small> {row.versions.length} builds</small>}</td><td><span className={'package-state'+(row.selection?.enabled?' enabled':'')}>{tab==='installed'?(row.selection?.enabled?'Enabled':'Disabled'):kind(row.artifact)}</span></td></tr>)}</tbody></table>
     {!rows.length&&<div className="packages-empty"><Icon name={tab==='installed'?'grid':'search'} size={32}/><h3>{search||filter!=='all'?'No matching packages':tab==='installed'?'No extensions installed':'The library is empty'}</h3><p>{search||filter!=='all'?'Try a different name, version or type.':tab==='installed'?`Add an extension to ${scopeTitle}.`:'Import an RTW file or drop it here.'}</p>{!search&&filter==='all'&&tab==='installed'&&<Button disabled={busy||!ready} onClick={browse}>Browse local library</Button>}</div>}
    </div>
    {next&&tab==='library'&&<div className="library-more"><Button disabled={busy} onClick={()=>void read(async()=>{const page=await domain.current!.request<Page>('inventory',{after:next});if(mounted.current){setArtifacts(old=>[...new Map([...old,...page.items].map(item=>[item.digest,item])).values()]);setNext(page.next);}})}>Load more packages</Button></div>}
   </section>
   <aside className="package-details" aria-label="Selected package">{candidate?<><div className="details-icon"><Icon name={kind(candidate)==='World template'?'world':'grid'} size={30}/></div><h2>{candidate.name}</h2><span className="details-kind">{kind(candidate)}</span>
    {selected!.versions.length>1?<label className="version-picker">Version<select aria-label="Package version" value={candidate.digest} disabled={busy} onChange={event=>setChosenVersion(event.target.value)}>{selected!.versions.map(item=><option key={item.digest} value={item.digest}>{versionLabel(item)}</option>)}</select></label>:<p className="single-version">{candidate.version??'Unversioned archive'}</p>}
    {candidate.error&&<Status error>{candidate.error}</Status>}
    <div className="details-actions">{isExtension(candidate)&&!candidate.error&&(installedCandidate?<><div className="installed-label">Installed in {scopeTitle}</div><Button disabled={changesBlocked} onClick={()=>void mutate('set-enabled',{scopeId:scope,subject:installedCandidate.subject,enabled:!installedCandidate.enabled},`${candidate.name} ${installedCandidate.enabled?'disabled':'enabled'}.`)}>{installedCandidate.enabled?'Disable':'Enable'}</Button><Button className="danger" disabled={changesBlocked} onClick={()=>void mutate('remove',{scopeId:scope,subject:installedCandidate.subject},`${candidate.name} removed from ${scopeTitle}. Its data and archive are kept.`)}>Remove from scope</Button></>:<Button className="primary" disabled={changesBlocked} onClick={()=>void inspect(candidate)}>{installedPackage?'Switch version':'Install'}</Button>)}
    {kind(candidate)==='World template'&&!candidate.error&&<><Button className="primary" disabled={busy||!ready} onClick={()=>void createWorld(candidate)}>Create World…</Button><p>Creates an independent World. Review its setup in the next window.</p></>}
    {!isExtension(candidate)&&kind(candidate)!=='World template'&&!candidate.error&&<p>This package type cannot be installed here yet.</p>}
    </div>
    {isExtension(candidate)&&<p className="details-note">{installedCandidate?'Removing this extension keeps its archive and saved World data.':'Installation changes this target only. Requested access is reviewed before selection.'}</p>}
    <details className="package-technical"><summary>Package details</summary>{candidate.subject&&<><h3>Package ID</h3><code>{candidate.subject}</code></>}<h3>{candidate.digest.startsWith('sha256:')?'Artifact digest':'Catalog entry'}</h3><code>{candidate.digest}</code><p>Catalog labels are hints. Exact bytes are verified before selection.</p></details>
   </>:<div className="details-empty"><Icon name="grid" size={30}/><p>Select a package to see its versions and actions.</p></div>}</aside>
  </div>
  <footer className="packages-status" role="status"><span className={busy?'working':''}/><p title={status}>{status}</p>{!ready&&<Button disabled={busy} onClick={()=>void allow()}>Allow access</Button>}{unknown&&<Button disabled={busy} onClick={()=>void refresh()}>Refresh</Button>}</footer>
  {dragging&&<div className="package-drop-hint"><Icon name="plus" size={36}/><h2>Drop RTW packages to import</h2></div>}
  <Dialog open={!!review} title="Install extension" onClose={()=>{if(!busy)setReview(null);}}>{review&&<><h3>{review.artifact.name} <small>{review.artifact.version}</small></h3><p>Install in <strong>{review.scopeTitle}</strong>. It will use this exact version: <code title={review.artifact.digest}>{fingerprint(review.artifact.digest)}</code>.</p><div className="install-permissions"><h4>Requested runtime access</h4>{review.components.map(component=><div key={component.componentId}><strong>{component.componentId}</strong><p>{component.requested.join(', ')||'No runtime permissions requested'}</p></div>)}</div><p className="install-note">New permissions are not approved automatically. Selecting a version does not migrate saved data.</p><footer><Button disabled={busy} onClick={()=>setReview(null)}>Cancel</Button><Button className="primary" disabled={busy||unknown} onClick={()=>void mutate('select',{scopeId:review.scope,digest:review.artifact.digest},`${review.artifact.name} installed in ${review.scopeTitle}.`,review.artifact.subject??undefined)}>Install</Button></footer></>}</Dialog>
 </main>;
}
createRoot(document.getElementById('root')!).render(<Packages/>);
