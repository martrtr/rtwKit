import {test} from 'node:test';
import assert from 'node:assert/strict';
import {discover,search,labels} from '../src/catalogs.ts';
const p={instanceId:'third.party',componentId:'custom-catalog',activation:'7'};
const other={...p,activation:'8'};
const id='00000000-0000-4000-8000-000000000001';
const reply=(value,provider=p)=>({provider,payload:new TextEncoder().encode(JSON.stringify({ok:true,value}))});
test('catalogs use attested identity and retain third-party additions',async()=>{
 const c={querySourced:async()=>[reply({catalogs:[{id:'tools',label:'Tools',order:1,extra:true}]}),reply({catalogs:[{id:'tools',label:'Other Tools',order:2}]},other)]};
 const v=await discover(c);assert.equal(v.items.length,2);assert.notEqual(v.items[0].key,v.items[1].key);
});
test('duplicate declarations from one provider are not arbitrarily selected',async()=>{
 const r=reply({catalogs:[{id:'tools',label:'Tools',order:0}]});const v=await discover({querySourced:async()=>[r,r]});assert.equal(v.items.length,0);assert.equal(v.rejected,1);
});
test('search rejects forged source, malformed cursor and duplicate matching replies',async()=>{
 const catalog={id:'tools',label:'Tools',order:0,key:'x',provider:p};const signal=new AbortController().signal;
 await assert.rejects(search({querySourced:async()=>[reply({items:[],next:null},other)]},catalog,'',null,signal),/unavailable/);
 const r=reply({items:[],next:id});assert.deepEqual(await search({querySourced:async()=>[r]},catalog,'',null,signal),{items:[],next:id});
 await assert.rejects(search({querySourced:async()=>[r]},catalog,'',id,signal),/cursor/);
 await assert.rejects(search({querySourced:async()=>[r,r]},catalog,'',null,signal),/unavailable/);
});
test('descriptions never inject unrequested items; missing catalogs preserve references',async()=>{
 assert.equal((await labels({querySourced:async()=>[reply({items:[{itemId:id,label:'Injected',detail:''}]})]},[],new AbortController().signal)).size,0);
 assert.equal((await labels({querySourced:async()=>[]},[id],new AbortController().signal)).size,0);
});
