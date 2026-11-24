import test from 'node:test';
import assert from 'node:assert/strict';
import { messageActions,invokeMessageAction } from '../src/actions.ts';
const provider={instanceId:'owner',componentId:'backend',activation:'7'};
const descriptor={actionPoint:'owner/action/v1',actions:[{id:'inspect',label:'Inspect',order:0}]};
const reply=(value,source=provider)=>({provider:source,payload:new TextEncoder().encode(JSON.stringify(value))});
const envelope=value=>[new TextEncoder().encode(JSON.stringify({ok:true,value}))];
const context={conversationId:'00000000-0000-7000-8000-000000000001',messageId:'00000000-0000-7000-8000-000000000002',expectedRevision:'1'};
test('backend actions retain only Native source and reject duplicate/malformed descriptors as a whole',async()=>{
 let value=await messageActions({querySourced:async()=>[reply(descriptor)]});assert.deepEqual(value.items[0].provider,provider);assert.equal(value.items[0].point,descriptor.actionPoint);
 for(const replies of [[reply(descriptor),reply(descriptor)],[reply({...descriptor,instanceId:'forged'})],[reply({...descriptor,actionPoint:'other/**'})],[reply({...descriptor,actions:[descriptor.actions[0],descriptor.actions[0]]})],[reply({...descriptor,actions:[{id:'x',label:'x',order:Infinity}]})]]){value=await messageActions({querySourced:async()=>replies});assert.deepEqual(value.items,[]);assert.ok(value.rejected);}
});
test('invocation is directed to discovered principal and inert captured context without fanout',async()=>{
 const action=(await messageActions({querySourced:async()=>[reply(descriptor)]})).items[0];const calls=[];
 const client={queryTo:async(point,payload,expected)=>{calls.push({point,call:JSON.parse(new TextDecoder().decode(payload)),expected});return envelope({status:'completed',message:'Done'});},query:()=>assert.fail('fanout')};
 assert.equal(await invokeMessageAction(client,action,context,new AbortController().signal),'Done');assert.deepEqual(calls,[{point:descriptor.actionPoint,call:{method:'invoke',actionId:'inspect',context},expected:provider}]);
});
test('unknown invocation never retries; released pending result stays tied to original operation',async()=>{
 const action=(await messageActions({querySourced:async()=>[reply(descriptor)]})).items[0];let begins=0;
 await assert.rejects(invokeMessageAction({queryTo:async()=>{begins++;throw Error('lost');}},action,context,new AbortController().signal),/lost/);assert.equal(begins,1);
 const calls=[],operation='x'.repeat(43);const controller=new AbortController();
 const client={queryTo:async(_p,bytes)=>{const call=JSON.parse(new TextDecoder().decode(bytes));calls.push(call);if(call.method==='invoke'){controller.abort();return envelope({status:'pending',operation});}return envelope(null);}};
 await assert.rejects(invokeMessageAction(client,action,context,controller.signal),e=>e.name==='AbortError');await new Promise(r=>setTimeout(r,0));assert.deepEqual(calls.map(c=>c.method),['invoke','release']);assert.equal(calls[1].operation,operation);
});
