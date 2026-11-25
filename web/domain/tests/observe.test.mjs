import test from 'node:test';
import assert from 'node:assert/strict';
import { Client } from '../../bridge/src/index.ts';
import { observeResource } from '../src/observe.ts';
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=Date.now()+2000;while(Date.now()<end){if(await check())return;await pause(5);}throw Error('condition timed out');}
const id='00000000-0000-7000-8000-000000000001';
const hint=new TextEncoder().encode(JSON.stringify({v:1,resource:id}));
function fixture(){let messages=[],next=0;const calls=[];const client=new Client(async(method,args)=>{calls.push(method);if(method==='transport.subscribe')return String(++next);if(method==='transport.poll'){const out=messages;messages=[];return out.map(bytes=>Buffer.from(bytes).toString('base64'));}return null;});return {client,calls,send:values=>messages.push(...values)};}
test('subscribe precedes read; a flood during an in-flight read coalesces without concurrent or reordered reads',async()=>{
 const f=fixture(),values=[];let resolve,reads=0,active=0,maximum=0;
 const watch=observeResource({client:f.client,points:['owner/changed'],resource:id,interval:10000,
  read:async()=>{reads++;maximum=Math.max(maximum,++active);const result=reads===1?await new Promise(r=>resolve=r):2;active--;return result;},apply:v=>values.push(v),failed:error=>{throw error;}});
 try{await until(()=>reads===1);assert.equal(f.calls[0],'transport.subscribe');f.send(Array(1000).fill(hint));await pause(230);resolve(1);await until(()=>values.includes(2));assert.deepEqual(values,[1,2]);assert.equal(maximum,1);assert.equal(reads,2);}finally{watch.close();}await pause(10);assert.equal(f.calls.filter(v=>v==='transport.unsubscribe').length,1);
});
test('lost final hint converges by periodic source reads',async()=>{
 let source=1;const values=[];const watch=observeResource({interval:20,read:async()=>source,apply:v=>values.push(v),failed:assert.fail});
 try{await until(()=>values.includes(1));source=2;await until(()=>values.includes(2));}finally{watch.close();}
});
test('read failure keeps last data and rejects explicit reconciliation without replaying a write',async()=>{
 let fail=false,reads=0,current;const errors=[];const watch=observeResource({interval:10000,read:async()=>{reads++;if(fail)throw Error('offline');return ['saved'];},apply:v=>current=v,failed:e=>errors.push(e.message)});
 try{await watch.refresh();fail=true;await assert.rejects(watch.refresh(),/offline/);assert.deepEqual(current,['saved']);assert.deepEqual(errors,['offline']);const n=reads;await pause(100);assert.equal(reads,n);}finally{watch.close();}
});
test('retired selection cannot apply a non-cooperating late result',async()=>{
 let complete;const seen=[];const first=observeResource({read:()=>new Promise(resolve=>complete=resolve),apply:v=>seen.push(v),failed:assert.fail});
 await until(()=>!!complete);first.close();const second=observeResource({read:async()=> 'new conversation',apply:v=>seen.push(v),failed:assert.fail});
 try{await second.refresh();complete('old conversation');await pause(20);assert.deepEqual(seen,['new conversation']);}finally{second.close();}
});
test('hidden views stop reads, discard in-flight data and reconcile on return',async()=>{
 const f=fixture();f.client.visibilityChanged(false);let reads=0,complete;const seen=[];
 const watch=observeResource({client:f.client,interval:20,read:async()=>{reads++;return reads===1?new Promise(resolve=>complete=resolve):'current';},apply:v=>seen.push(v),failed:assert.fail});
 try{await pause(50);assert.equal(reads,0);f.client.visibilityChanged(true);await until(()=>!!complete);f.client.visibilityChanged(false);complete('hidden result');await pause(40);assert.deepEqual(seen,[]);const n=reads;await pause(50);assert.equal(reads,n);f.client.visibilityChanged(true);await until(()=>seen.includes('current'));}finally{watch.close();}
});
test('invalid and unrelated hints do not trigger reads; pending subscription is released on close',async()=>{
 const f=fixture();let reads=0;const watch=observeResource({client:f.client,points:['x'],resource:id,interval:10000,read:async()=>++reads,apply:()=>{},failed:assert.fail});
 try{await until(()=>reads===1);f.send([new TextEncoder().encode('{'),new Uint8Array(257),new TextEncoder().encode(JSON.stringify({v:1,resource:'00000000-0000-7000-8000-000000000002'})),new TextEncoder().encode(JSON.stringify({v:1,resource:null,authority:'forged'}))]);await pause(250);assert.equal(reads,1);}finally{watch.close();}
 let finish,unsubscribed=0;const late=new Client(async(method)=>method==='transport.subscribe'?new Promise(resolve=>finish=resolve):++unsubscribed);
 const pending=observeResource({client:late,points:['x'],read:async()=>assert.fail(),apply:assert.fail,failed:assert.fail});await until(()=>!!finish);pending.close();finish('3');await until(()=>unsubscribed===1);
});

test('continuous hints cannot starve successful authoritative reads',async()=>{
 const f=fixture(),values=[];let reads=0;
 const watch=observeResource({client:f.client,points:['x'],interval:10000,read:async()=>{const value=++reads;await pause(250);return value;},apply:value=>values.push(value),failed:assert.fail});
 const flood=setInterval(()=>f.send([hint]),50);
 try{await until(()=>values.length>=2);assert.deepEqual(values.slice(0,2),[1,2]);}finally{clearInterval(flood);watch.close();}
});
