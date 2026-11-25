import assert from 'node:assert/strict';
import test from 'node:test';
import { BridgeError } from '@rintawa/bridge';
import { uploadPackage, savePackage, MAX_BLOB_BYTES } from '../../../web/ui/src/packages.ts';

test('package upload sends bounded chunks with exact offsets and never retries an unknown write', async () => {
  const calls=[];let offset=0;
  const client={request:async(method,args)=>{calls.push([method,args]);if(method==='packages.begin-upload')return '9';if(method==='packages.append-upload'){assert.equal(args.offset,String(offset));const bytes=Buffer.from(args.bytes,'base64');assert.ok(bytes.length<=32768);offset+=bytes.length;return String(offset);}if(method==='packages.status')return {status:'installed',digest:'sha256:'+'a'.repeat(64)};return null;}};
  const file=new File([new Uint8Array(70000)],'template.rtw');
  assert.equal(await uploadPackage(client,file,new AbortController().signal,()=>{}),'sha256:'+'a'.repeat(64));
  assert.equal(offset,file.size);assert.equal(calls.at(-1)[0],'packages.close');
  const failed=[];const unknown={request:async(method)=>{failed.push(method);if(method==='packages.begin-upload')return '1';if(method==='packages.append-upload')throw new BridgeError('unavailable','unknown');return null;}};
  await assert.rejects(uploadPackage(unknown,file,new AbortController().signal,()=>{}),BridgeError);
  assert.deepEqual(failed,['packages.begin-upload','packages.append-upload','packages.close']);
});
test('streamed export reads immutable offsets, aborts failed writers and bounds Blob fallback', async () => {
  const bytes=Buffer.alloc(70000,7);const writes=[];let closed=false,aborted=false;
  const client={request:async(method,args)=>{assert.equal(method,'packages.read');return bytes.subarray(Number(args.offset),Number(args.offset)+args.maximumBytes).toString('base64');}};
  const writer={write:async value=>writes.push(value),close:async()=>{closed=true;},abort:async()=>{aborted=true;}};
  await savePackage(client,'1',bytes.length,'world.rtw',{createWritable:async()=>writer},new AbortController().signal);
  assert.equal(closed,true);assert.equal(aborted,false);assert.deepEqual(Buffer.concat(writes),bytes);
  await assert.rejects(savePackage(client,'1',MAX_BLOB_BYTES+1,'world.rtw',null,new AbortController().signal),/streaming/);
  writer.write=async()=>{throw Error('disk full');};
  await assert.rejects(savePackage(client,'1',bytes.length,'world.rtw',{createWritable:async()=>writer},new AbortController().signal),/disk full/);assert.equal(aborted,true);
});
