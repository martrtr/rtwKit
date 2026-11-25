import { useEffect, useState, type MutableRefObject } from 'react';
import { DomainClient, DomainError } from '@rintawa/domain';
import { BridgeError } from '@rintawa/bridge';
import type { Config, Connection } from './main';
type Inventory = { status: 'completed'; models: string[]; omitted: number };
type Result = Inventory | { status: 'failed'; reason: string } | { status: 'pending' };
export type Probe = { state: string; models: string[]; detail: string; omitted?: number };
export type ProbeQueue = MutableRefObject<Promise<void>>;
const waiting: Probe = { state: 'Checking', models: [], detail: 'Checking the provider model list…' };
const inactive: Probe = { state: 'Not checked', models: [], detail: 'Open the provider to check its connection.' };
export function probeFailure(code: string): Probe {
  const details: Record<string, [string, string]> = {
    'http-failed': ['Offline', 'Connection failed. Check the server address and whether it is running.'],
    'http-timeout': ['Offline', 'The server did not respond before the timeout.'],
    'http-permission-required': ['Permission required', 'Choose Allow access to connect this extension to the server.'],
    'secret-read-permission-required': ['Key permission required', 'Choose Allow access to let this extension read the saved API key.'],
    'credentials-required': ['API key required', 'Store this provider’s API key in system credentials.'],
    'credential-store-unavailable': ['Key unavailable', 'The system credential store is unavailable or locked.'],
    'credentials-rejected': ['Authentication failed', 'The server rejected the API key.'],
    'save-connection-first': ['Save first', 'Save the provider before loading models with its API key. A model ID can be entered manually.'],
    'invalid-configuration': ['Not checked', 'Enter a valid API base URL.'],
    'invalid-model-list': ['Incompatible response', 'The server did not return a compatible model list. Enter a model ID manually.'],
    'model-list-too-large': ['Response too large', 'The server model list exceeds the supported limit.'],
    'model-http-status': ['Request rejected', 'The server rejected model discovery. Enter a model ID manually.'],
    'rate-limited': ['Rate limited', 'Try refreshing later.'],
  };
  const [state, detail] = details[code] ?? ['Unavailable', 'The check could not complete. Refresh to try again.'];
  return { state, detail, models: [] };
}
// One mounted Settings page serializes automatic discovery; no background monitor.
export function useModels(domain: DomainClient | null, config: Config | undefined, expected: Connection | undefined, enabled: boolean, revision: number, queue: ProbeQueue): Probe {
  const key = JSON.stringify([config, expected, enabled, revision, !!domain]);
  const [value, setValue] = useState<{ key: string; probe: Probe } | null>(null);
  useEffect(() => {
    if (!domain || !config || !enabled) return;
    let disposed = false; let operation: string | undefined; let releasing: Promise<void> | undefined;
    const release = () => { if (operation) { const id = operation; operation = undefined; releasing = domain.request('release', { operation: id }).then(() => {}, () => {}); } return releasing ?? Promise.resolve(); };
    const apply = (probe: Probe) => { if (!disposed) setValue({ key, probe }); };
    const run = async () => {
      if (disposed) return;
      try {
        const admitted = await domain.request<{ operation: string }>('begin-models', { config, ...(expected ? { expected } : {}) });
        operation = admitted.operation;
        while (!disposed) {
          const result = await domain.request<Result>('status', { operation });
          if (result.status === 'completed') { apply({ state: 'Online', detail: `Last check succeeded. ${result.models.length} models available.`, models: result.models, omitted: result.omitted }); return; }
          if (result.status === 'failed') { apply(probeFailure(result.reason)); return; }
          await new Promise(resolve => setTimeout(resolve, 80));
        }
      } catch (error) {
        apply(probeFailure(error instanceof DomainError || error instanceof BridgeError ? error.code : 'unavailable'));
      } finally { await release(); }
    };
    const timer = setTimeout(() => { queue.current = queue.current.then(run, run); }, 400);
    return () => { disposed = true; clearTimeout(timer); void release(); };
  }, [domain, key, queue]);
  if (!enabled) return inactive;
  if (!config?.baseUrl.trim()) return { ...inactive, detail: 'Enter an API base URL to load models.' };
  return value?.key === key ? value.probe : waiting;
}
export function ConnectionState({ probe }: { probe: Probe }) {
  return <span className={`ai-connectivity ${probe.state.toLowerCase().replaceAll(' ', '-')}`} title={probe.detail} role="status"><i />{probe.state}</span>;
}
