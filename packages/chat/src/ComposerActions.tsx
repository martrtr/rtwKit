import { useEffect, useState } from 'react';
import { observeResource } from '@rintawa/domain';
import type { Client } from '@rintawa/bridge';
import { Button, Status } from '@rintawa/ui';
import { viewContributions, type ViewContribution } from '../../../web/ui/src/contributions';
export function ComposerActions({client}:{client:Client|null}) {
 const [items,setItems]=useState<ViewContribution[]>([]),[error,setError]=useState('');
 useEffect(()=>{
  if(!client)return;
  const watch=observeResource({client,read:signal=>viewContributions(client,'rintawa/chat/composer-actions/v1','label',signal),
   apply:next=>{setItems(next.items);setError(next.rejected?'Some actions are invalid or ambiguous.':'');},
   failed:error=>setError(error instanceof Error?error.message:'Actions unavailable.')});
  return ()=>watch.close();
 },[client]);
 return <div className="composer-contributions">{items.map(item=><Button key={item.id} title={item.label} onClick={()=>void client?.navigateView(item.view).catch(error=>setError(error.message))}>{item.label}</Button>)}{error&&<Status error>{error}</Status>}</div>;
}
