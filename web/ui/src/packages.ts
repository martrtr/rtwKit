import { BridgeError, decodeBytes, encodeBytes, handle, type Client } from '@rintawa/bridge';
export type PackageStatus = { status: 'uploading'; written: string } | { status: 'pending' } | { status: 'installed'; digest: string } | { status: 'download'; size: string } | { status: 'failed'; reason: string };
export const MAX_PACKAGE_BYTES = 1024 * 1024 * 1024;
export const MAX_BLOB_BYTES = 64 * 1024 * 1024;
export function checkCancelled(signal?: AbortSignal) { if (signal?.aborted) throw new DOMException('Cancelled', 'AbortError'); }
export async function pause(signal?: AbortSignal) { checkCancelled(signal); await new Promise(resolve => setTimeout(resolve, 80)); checkCancelled(signal); }
/** Retrying a read never resubmits an upload, installation or export. */
export async function packageRead<T>(client: Pick<Client, 'request'>, method: string, args: Record<string, unknown> = {}, signal?: AbortSignal): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    checkCancelled(signal);
    try { return await client.request(method, args) as T; }
    catch (error) { if (attempt >= 8 || !(error instanceof BridgeError) || !['unavailable', 'limit-exceeded'].includes(error.code)) throw error; await pause(signal); }
  }
}
export async function closePackage(client: Pick<Client, 'request'>, id: string): Promise<void> {
  for (let attempt = 0; ; attempt++) {
    try { await client.request('packages.close', { handle: id }); return; }
    catch (error) {
      if (error instanceof BridgeError && error.code === 'not-found') return;
      if (attempt >= 5 || !(error instanceof BridgeError) || !['unavailable', 'limit-exceeded'].includes(error.code)) throw error;
      await pause();
    }
  }
}
export async function waitPackage(client: Pick<Client, 'request'>, id: string, signal?: AbortSignal): Promise<PackageStatus> {
  for (;;) {
    const result = await packageRead<PackageStatus>(client, 'packages.status', { handle: id }, signal);
    if (result.status === 'failed') throw new Error(result.reason);
    if (result.status !== 'pending' && result.status !== 'uploading') return result;
    await pause(signal);
  }
}
export async function uploadPackage(client: Pick<Client, 'request'>, file: File, signal: AbortSignal, progress: (percent: number) => void): Promise<string> {
  if (!file.size || file.size > MAX_PACKAGE_BYTES || !file.name.toLowerCase().endsWith('.rtw')) throw new Error('Choose a nonempty .rtw file up to 1 GiB.');
  checkCancelled(signal);
  const id = handle(await client.request('packages.begin-upload', { size: String(file.size) }));
  try {
    for (let offset = 0; offset < file.size;) {
      checkCancelled(signal);
      const bytes = new Uint8Array(await file.slice(offset, offset + 32768).arrayBuffer());
      const written = handle(await client.request('packages.append-upload', { handle: id, offset: String(offset), bytes: encodeBytes(bytes) }));
      offset += bytes.length; if (written !== String(offset)) throw new Error('Unexpected package upload offset');
      progress(Math.floor(offset * 100 / file.size));
    }
    checkCancelled(signal);
    await client.request('packages.finish-upload', { handle: id });
    const result = await waitPackage(client, id, signal);
    if (result.status !== 'installed' || !/^sha256:[0-9a-f]{64}$/.test(result.digest)) throw new Error('Invalid package result');
    return result.digest;
  } finally { await closePackage(client, id).catch(() => {}); }
}
export type SaveHandle = { createWritable(): Promise<{ write(bytes: Uint8Array): Promise<void>; close(): Promise<void>; abort(): Promise<void> }> };
export type SavePicker = (options: { suggestedName: string; types: { description: string; accept: Record<string, string[]> }[] }) => Promise<SaveHandle>;
export async function savePackage(client: Pick<Client, 'request'>, id: string, size: number, filename: string, target: SaveHandle | null, signal: AbortSignal): Promise<void> {
  if (!Number.isSafeInteger(size) || size < 1 || size > MAX_PACKAGE_BYTES) throw new Error('Invalid export size');
  if (!target && size > MAX_BLOB_BYTES) throw new Error('This browser needs a streaming file picker for exports above 64 MiB. Use a supported browser or the CLI.');
  const writer = target ? await target.createWritable() : null;
  const chunks: Uint8Array<ArrayBuffer>[] = [];
  try {
    for (let offset = 0; offset < size;) {
      const bytes = decodeBytes(await packageRead(client, 'packages.read', { handle: id, offset: String(offset), maximumBytes: Math.min(32768, size - offset) }, signal));
      if (!bytes.length || bytes.length > Math.min(32768, size - offset)) throw new Error('Incomplete export download');
      if (writer) await writer.write(bytes); else chunks.push(bytes);
      offset += bytes.length;
    }
    checkCancelled(signal);
    if (writer) await writer.close();
    else {
      const url = URL.createObjectURL(new Blob(chunks, { type: 'application/octet-stream' }));
      const link = document.createElement('a'); link.href = url; link.download = filename; document.body.append(link); link.click(); link.remove();
      setTimeout(() => URL.revokeObjectURL(url), 60_000);
    }
  } catch (error) { await writer?.abort().catch(() => {}); throw error; }
}
