import { useEffect, useState } from 'react';
import { BridgeError, resolveView, type RootBridge, type View, type PermissionRequest } from '@rintawa/bridge';
import { Button, Dialog, Status } from '@rintawa/ui';
type Snapshot = { target: { scopeId: string; instanceId: string; componentId: string; artifact: string; activation: string }; requested: string[]; requestedSecretRead: string[]; granted: string[]; httpOrigins: string[]; secretRead: string[] };
type Choice = { kind: 'runtime' | 'origin' | 'secret'; value: string; label: string; checked: boolean };
const labels: Record<string, string> = { 'http-request': 'Connect to servers', 'published-preference-read': 'Read shared configuration', 'published-preference-write': 'Change shared settings for all worlds', 'user-resource-read': 'Read selected files', 'background-task': 'Run background work' };
export function Permissions({ bridge, source, scopeTitle, close }: { bridge: RootBridge; source: { view: View; request: PermissionRequest }; scopeTitle: string; close: () => void }) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null); const [choices, setChoices] = useState<Choice[]>([]);
  const [busy, setBusy] = useState(false); const [error, setError] = useState(''); const [unknown, setUnknown] = useState(false); const [attempt, setAttempt] = useState(0);
  const { view, request } = source;
  useEffect(() => {
    let disposed = false; setSnapshot(null); setError('');
    void (async () => {
      let state: Snapshot | undefined;
      for (let i = 0; !state; i++) {
        try { state = await bridge.client.request('runtime-policy.live-component', { scopeId: view.worldId ? `world:${view.worldId}` : 'host', instanceId: view.restoreKey.instanceId, componentId: request.componentId }) as Snapshot; }
        catch (failure) { if (i >= 9 || !(failure instanceof BridgeError) || failure.code !== 'unavailable') throw failure; await new Promise(r => setTimeout(r, 100)); }
        if (disposed) return;
      }
      if (request.runtime.some(r => !state!.requested.includes(r)) || request.secretRead.some(path => !state!.requestedSecretRead.some(pattern => pattern === path || (pattern.endsWith('.*') && path.startsWith(pattern.slice(0, -1)))))) throw new Error('The component did not declare the requested access.');
      const pending: Choice[] = [
        ...request.runtime.filter(r => !state!.granted.includes(r)).map(value => ({ kind: 'runtime' as const, value, label: labels[value] ?? value, checked: true })),
        ...request.httpOrigins.filter(r => !state!.httpOrigins.includes(r)).map(value => ({ kind: 'origin' as const, value, label: `Access local server: ${value}`, checked: true })),
        ...request.secretRead.filter(r => !state!.secretRead.includes(r)).map(value => ({ kind: 'secret' as const, value, label: 'Read a saved credential', checked: true })),
      ];
      if (disposed) return;
      if (!pending.length) { bridge.notifyPermissionsChanged(view.id); close(); return; }
      setSnapshot(state); setChoices(pending); setUnknown(false);
    })().catch(failure => { if (!disposed) setError(failure instanceof Error ? failure.message : 'Access unavailable'); });
    return () => { disposed = true; };
  }, [bridge, source, attempt]);
  const apply = async () => {
    if (!snapshot) return;
    setBusy(true); setError('');
    try {
      const current = resolveView(view.restoreKey, await bridge.list());
      if (!current || current.id !== view.id) throw new Error('The requesting extension stopped. Reopen its settings.');
      const change = { target: snapshot.target, delta: {
        grantRuntime: choices.filter(c => c.checked && c.kind === 'runtime').map(c => c.value),
        allowHttpOrigins: choices.filter(c => c.checked && c.kind === 'origin').map(c => c.value),
        allowSecretRead: choices.filter(c => c.checked && c.kind === 'secret').map(c => c.value),
      } };
      await bridge.client.request('runtime-policy.apply-live-change', { change });
      bridge.notifyPermissionsChanged(view.id); close();
    } catch (failure) { setUnknown(true); setError(`${failure instanceof Error ? failure.message : 'Access update unavailable'}. Check current access before making another decision.`); }
    finally { setBusy(false); }
  };
  const missingHttp = !snapshot?.granted.includes('http-request') && !choices.some(c => c.kind === 'runtime' && c.value === 'http-request' && c.checked);
  return <Dialog open title="Allow access" onClose={() => { if (!busy) close(); }}>
    <p><strong>{view.title}</strong> requests access for <strong>{scopeTitle}</strong>.</p>
    {snapshot ? <><ul className="permission-summary">{choices.map(c => <li key={`${c.kind}/${c.value}`}>{c.label}</li>)}</ul><details><summary>Choose individual permissions</summary><p className="dialog-description">{snapshot.target.instanceId}/{snapshot.target.componentId}</p><code className="permission-artifact">{snapshot.target.artifact}</code>{choices.map((c, index) => <label className="permission-choice" key={`${c.kind}/${c.value}`}><input type="checkbox" disabled={busy || unknown} checked={c.checked} onChange={e => setChoices(previous => previous.map((item, i) => i === index ? { ...item, checked: e.target.checked } : item))} />{c.label}{c.kind === 'secret' && <code>{c.value}</code>}</label>)}</details><p className="dialog-description">Applies immediately to this extension. Other Worlds keep their own permissions.</p></> : !error && <Status>Checking requested access…</Status>}
    {error && <Status error>{error}</Status>}
    <footer><Button disabled={busy} onClick={close}>Cancel</Button>{unknown ? <Button onClick={() => setAttempt(v => v + 1)}>Check current access</Button> : <Button className="primary" disabled={busy || !snapshot || !choices.some(c => c.checked) || (missingHttp && choices.some(c => c.kind === 'origin' && c.checked))} onClick={() => void apply()}>Allow selected</Button>}</footer>
  </Dialog>;
}
