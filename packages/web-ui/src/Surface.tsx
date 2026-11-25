import { useEffect, useRef, useState } from 'react';
import { type RootBridge, type View } from '@rintawa/bridge';
import { Status } from '@rintawa/ui';
export function selectedView(views: View[], scope: string | null, contract: string): View | null {
 const candidates=views.filter(view=>view.worldId===scope&&view.provides.some(p=>p.id===contract&&p.version===1&&p.selected));return candidates.length===1?candidates[0]:null;
}
/** Root owns the viewport/port; presentation and discovery run in the selected extension. */
export function Surface({bridge,view,label}:{bridge:RootBridge;view:View|null;label:string}) {
 const element=useRef<HTMLDivElement>(null);const [error,setError]=useState('');
 useEffect(()=>{if(!view||!element.current)return;let disposed=false,mount:{close:()=>Promise<void>}|undefined;const abort=new AbortController();setError('');bridge.mount(view.id,element.current,()=>{if(!disposed)setError(`${label} disconnected.`);},{},abort.signal).then(value=>{if(disposed)void value.close().catch(()=>{});else mount=value;}).catch(error=>{if(!disposed)setError(error instanceof Error?error.message:`${label} unavailable.`);});return()=>{disposed=true;abort.abort();void mount?.close().catch(()=>{});};},[bridge,view?.id,view?.activation,label]);
 return <div className="selected-surface"><div ref={element} className="surface-frame"/>{(!view||error)&&<div className="surface-placeholder"><Status error>{error||`No unique selected ${label} provider is active in this scope.`}</Status></div>}</div>;
}
