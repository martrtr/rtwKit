import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { observeResource, type Observation } from '@rintawa/domain';
import { connectPanel, type Client } from '@rintawa/bridge';
import { Button, Icon, Status } from '@rintawa/ui';
import logo from './assets/logo.png';
import '@rintawa/ui/style.css';
import './home.css';
type World = {worldId:string;title:string;description:string|null;active:boolean};
function App() {
 const client=useRef<Client|null>(null);const [worlds,setWorlds]=useState<World[]>([]),[search,setSearch]=useState(''),[loaded,setLoaded]=useState(false),[error,setError]=useState('');
 const watch=useRef<Observation|null>(null);
 useEffect(()=>{let disposed=false;connectPanel().then(value=>{if(disposed){value.dispose();return;}client.current=value;
  watch.current=observeResource({client:value,read:async()=>await value.request('world-sessions.list-worlds') as World[],
   apply:rows=>{setWorlds(rows);setLoaded(true);setError('');},failed:error=>setError(error instanceof Error?error.message:'Worlds unavailable.')});
 }).catch(error=>setError(error.message));return()=>{disposed=true;watch.current?.close();client.current?.dispose();};},[]);
 const action=(method:string,args={})=>void client.current?.request(method,args).catch(error=>setError(error.message));
 const filtered=worlds.filter(world=>world.title.toLowerCase().includes(search.toLowerCase()));
 return <main className="home-content"><div className="home-intro"><img src={logo} alt=""/><h1>Welcome to Rintawa.</h1></div><div className="worlds-heading"><h2>Your worlds</h2><Button className="primary" disabled={!loaded} onClick={()=>action('ui.create-world')}>Create a world</Button></div>{!!worlds.length&&<label className="world-search"><Icon name="search" size={17}/><input value={search} onChange={event=>setSearch(event.target.value)} placeholder="Find a world" aria-label="Find a world"/></label>}{error&&<><Status error>{error}</Status><Button onClick={()=>void watch.current?.refresh().catch(()=>{})}>Retry reads</Button></>}{!loaded?<Status>Loading worlds…</Status>:filtered.length?<div className="world-grid">{filtered.map(world=><button className="world-card" key={world.worldId} onClick={()=>action('ui.open-world',{worldId:world.worldId})}><div className="world-card-icon">{world.title.trim().split(/\s+/).slice(0,2).map(word=>word[0]).join('').toUpperCase()}</div><div className="world-card-copy"><h3>{world.title}</h3>{world.description&&<p>{world.description}</p>}<span className={world.active?'world-state online':'world-state'}>{world.active?'Active':'Not running'}</span></div><Icon name="arrow" size={18}/></button>)}</div>:<div className="home-empty"><h3>{search?'No worlds found':'Your first world starts here'}</h3>{!search&&<Button className="primary" onClick={()=>action('ui.create-world')}>Create a world</Button>}</div>}</main>;
}
createRoot(document.getElementById('root')!).render(<App/>);
