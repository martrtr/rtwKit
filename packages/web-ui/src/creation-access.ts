import type { RootBridge } from '@rintawa/bridge';
export type CreationPermission = {slot:string;componentId:string;artifact:string;httpOrigins:string[];secretRead:string[];label:string};
export type TemplateComponent={slot:string;artifact:string;packageId:string|null;version:string|null};
const object=(v:unknown):v is Record<string,unknown>=>!!v&&typeof v==='object'&&!Array.isArray(v);
export async function creationAccess(bridge:RootBridge,components:TemplateComponent[]):Promise<CreationPermission[]> {
 const replies=await bridge.client.querySourced('rintawa/world-creation/permissions/v1',new TextEncoder().encode('{"method":"describe"}'));
 const permissions:CreationPermission[]=[];const seen=new Set<string>();
 for(const reply of replies){
  const value:unknown=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(reply.payload));
  if(!object(value)||Object.keys(value).sort().join(',')!=='componentId,httpOrigins,label,secretRead'||typeof value.componentId!=='string'||!value.componentId||value.componentId.length>128||typeof value.label!=='string'||!value.label.trim()||new TextEncoder().encode(value.label).length>128)throw Error('Invalid creation access request.');
  const lists=[value.httpOrigins,value.secretRead];for(const list of lists)if(!Array.isArray(list)||list.length>64||list.some(v=>typeof v!=='string'||!v||v.length>2048)||new Set(list).size!==list.length)throw Error('Invalid creation access request.');
  const source=await bridge.client.request('runtime-policy.live-component',{scopeId:'host',instanceId:reply.provider.instanceId,componentId:reply.provider.componentId}) as {target:{artifact:string;activation:string}};
  if(source.target.activation!==reply.provider.activation)throw Error('Creation access provider changed. Reopen creation.');
  if(!(value.httpOrigins as string[]).length&&!(value.secretRead as string[]).length)continue;
  for(const component of components.filter(c=>c.artifact===source.target.artifact)){
   const key=`${component.slot}/${value.componentId}`;if(seen.has(key))throw Error('Ambiguous creation access request.');seen.add(key);
   permissions.push({slot:component.slot,componentId:value.componentId,artifact:component.artifact,httpOrigins:value.httpOrigins as string[],secretRead:value.secretRead as string[],label:value.label});
  }
 }
 if(permissions.reduce((n,p)=>n+p.httpOrigins.length+p.secretRead.length,0)>64)throw Error('Too many creation permissions.');
 return permissions;
}
