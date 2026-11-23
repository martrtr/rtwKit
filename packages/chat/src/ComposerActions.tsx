import { useEffect, useState } from 'react';
import type { Client } from '@rintawa/bridge';
import { Button, Status } from '@rintawa/ui';
import { viewContributions, type ViewContribution } from '../../../web/ui/src/contributions';
export function ComposerActions({client}:{client:Client|null}) {
 const [items,setItems]=useState<ViewContribution[]>([]),[error,setError]=useState('');
 useEffect(()=>{if(!client)return;let disposed=false,timer:ReturnType<typeof setTimeout>|undefined;const read=async()=>{try{const next=await viewContributions(client,'rintawa/chat/composer-actions/v1','label');if(!disposed){setItems(next.items);setError(next.rejected?'Some actions are invalid or ambiguous.':'');}}catch(error){if(!disposed)setError(error instanceof Error?error.message:'Actions unavailable.');}finally{if(!disposed)timer=setTimeout(()=>void read(),1500);}};void read();return()=>{disposed=true;clearTimeout(timer);};},[client]);
 return <div className="composer-contributions">{items.map(item=><Button key={item.id} title={item.label} onClick={()=>void client?.navigateView(item.view).catch(error=>setError(error.message))}>{item.label}</Button>)}{error&&<Status error>{error}</Status>}</div>;
}
