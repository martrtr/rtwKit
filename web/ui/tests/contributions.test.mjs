import test from 'node:test';
import assert from 'node:assert/strict';
import {viewContributions} from '../src/contributions.ts';
const provider={instanceId:'a',componentId:'view',activation:'7'};
const view={id:'actual',worldId:'world-a',activation:'7',title:'Section',launchable:false,provides:[],restoreKey:{worldId:'world-a',instanceId:'a',componentId:'view'}};
const reply=(data,source=provider)=>({provider:source,payload:new TextEncoder().encode(JSON.stringify(data))});
test('only the exact native reply source can contribute its live view',async()=>{
 const replies=[reply({id:'good',title:'A',order:0}),reply({id:'forged',title:'Fake',order:0,instanceId:'b'}),reply({id:'stale',title:'Stale',order:0},{...provider,activation:'6'}),reply({id:'unknown',title:'Unknown',order:0},{...provider,instanceId:'b'})];
 const items=await viewContributions({querySourced:async()=>replies,views:async()=>[view]},'sections','title');
 assert.equal(items.items.length,1);assert.equal(items.items[0].view,view);assert.equal(items.items[0].label,'A');assert.equal(items.rejected,3);
});
test('duplicate, stopped, ambiguous and malformed contributions supply no route',async()=>{
 const good=reply({id:'good',label:'Dice',order:1});
 for(const [replies,views]of [[[good,good],[view]],[[good],[]],[[good],[view,{...view,id:'duplicate'}]],[[reply({id:'x',label:'x'.repeat(129),order:1})],[view]],[[reply({id:'x',label:'Unsafe',order:Infinity})],[view]]]){const result=await viewContributions({querySourced:async()=>replies,views:async()=>views},'actions','label');assert.deepEqual(result.items,[]);assert.ok(result.rejected>0);}
});
