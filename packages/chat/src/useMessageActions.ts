import { useEffect, useRef, useState } from 'react';
import type { Client } from '@rintawa/bridge';
import { observeResource } from '@rintawa/domain';
import { messageActions, invokeMessageAction, type MessageAction, type MessageContext } from '../../../web/ui/src/actions';
export function useMessageActions(client: Client | null, conversation: string | null) {
  const [discoveryError,setDiscoveryError]=useState('');
  const [items,setItems]=useState<MessageAction[]>([]),[error,setError]=useState(''),[notice,setNotice]=useState(''),[busy,setBusy]=useState(false);
  const active=useRef<AbortController|null>(null),epoch=useRef(0);
  useEffect(()=>{
    if(!client)return;
    const watch=observeResource({client,read:signal=>messageActions(client,signal),apply:value=>{setItems(value.items);setDiscoveryError(value.rejected?'Some message actions are unavailable.':'');},failed:error=>setDiscoveryError(error instanceof Error?error.message:'Message actions unavailable.')});
    return()=>watch.close();
  },[client]);
  useEffect(()=>{
    epoch.current++;active.current?.abort();active.current=null;setBusy(false);setNotice('');setError('');
    return()=>{epoch.current++;active.current?.abort();};
  },[client,conversation]);
  const invoke=async(action:MessageAction,context:MessageContext)=>{
    if(!client||active.current)return;
    const current=epoch.current,controller=new AbortController();active.current=controller;setBusy(true);setError('');setNotice('');
    try {const message=await invokeMessageAction(client,action,context,controller.signal);if(epoch.current===current)setNotice(message);}
    catch(error){if(epoch.current===current&&!controller.signal.aborted)setError(`Action could not complete: ${error instanceof Error?error.message:'unavailable'}. Check its state before trying again.`);}
    finally{if(epoch.current===current){active.current=null;setBusy(false);}}
  };
  return {items,error:error||discoveryError,notice,busy,invoke};
}
