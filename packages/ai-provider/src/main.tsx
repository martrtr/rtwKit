import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, BridgeError, type Client } from '@rintawa/bridge';
import { DomainClient, DomainError } from '@rintawa/domain';
import { Button, Dialog, Input, Status } from '@rintawa/ui';
import '@rintawa/ui/style.css';
import './style.css';
import { ConnectionState, useModels, type ProbeQueue } from './Models';
export type Config = { baseUrl: string; model: string; requiresApiKey: boolean; maxOutputTokens: number; temperature: number; thinking?: boolean };
type Kind = 'ollama' | 'openai-compatible';
export type Connection = { id: string; label: string; template: Kind; config: Config };
type Description = { worldId: string | null; instanceId: string; connections: Connection[] | null; catalogError: string | null; defaultConnectionId: string | null };
type Draft = { expected?: Connection; label: string; template: Kind; config: Config };
const quote = (value: string) => "'" + value.replaceAll("'", "'\\''") + "'";
const keyPath = (connection: Connection) => `ai.connection_${connection.id.replaceAll('-', '')}.api_key`;
const originFor = (base: string) => { try { const url = new URL(base); return url.protocol === 'http:' ? url.origin : null; } catch { return null; } };
const errorText = (error: unknown) => error instanceof Error ? error.message : 'Settings unavailable';
function ConnectionHealth({ domain, connection, enabled, revision, queue }: { domain: DomainClient | null; connection: Connection; enabled: boolean; revision: number; queue: ProbeQueue }) {
  const probe = useModels(domain, connection.config, connection, enabled, revision, queue);
  return <ConnectionState probe={probe} />;
}
function App() {
  const client = useRef<Client | null>(null); const domain = useRef<DomainClient | null>(null); const alive = useRef(true);
  const [info, setInfo] = useState<Description | null>(null); const [busy, setBusy] = useState(true);
  const [error, setError] = useState(''); const [notice, setNotice] = useState(''); const [uncertain, setUncertain] = useState(false); const [writeDenied, setWriteDenied] = useState(false);
  const queue = useRef(Promise.resolve()); const [revision, setRevision] = useState(0);
  const [adding, setAdding] = useState(false); const [draft, setDraft] = useState<Draft | null>(null); const [removing, setRemoving] = useState<Connection | null>(null);
  const read = async () => { const data = await domain.current!.request<Description>('describe'); if (alive.current) setInfo(data); };
  useEffect(() => {
    alive.current = true;
    connectPanel().then(async value => {
      if (!alive.current) { value.dispose(); return; }
      client.current = value; value.onPermissionsChanged(() => { if (alive.current) { setRevision(v => v + 1); setWriteDenied(false); setNotice('Access allowed.'); void read().catch(failure => setError(errorText(failure))); } }); domain.current = new DomainClient(value, 'rintawa/inference/v1');
      for (let attempt = 0; ; attempt++) {
        try { await read(); break; }
        catch (failure) { if (attempt >= 9 || !(failure instanceof DomainError || failure instanceof BridgeError) || !['unavailable', 'limit-exceeded'].includes(failure.code)) throw failure; }
        if (!alive.current) return;
        await new Promise(resolve => setTimeout(resolve, 100));
      }
    }).catch(failure => { if (alive.current) setError(errorText(failure)); }).finally(() => { if (alive.current) setBusy(false); });
    return () => { alive.current = false; client.current?.dispose(); };
  }, []);
  const mutate = async (method: string, args: Record<string, unknown> = {}) => {
    if (!domain.current) return;
    setBusy(true); setError(''); setNotice('');
    try {
      const result = await domain.current.request<Connection | null>(method, args);
      if (!alive.current) return;
      setDraft(null); setRemoving(null); setNotice('Saved.'); await read();
      if (result && (method === 'add-connection' || (method === 'edit-connection' && result.id !== (args.expected as Connection).id))) await requestAccess(result);
    } catch (failure) {
      if (alive.current) {
        const rejected = (failure instanceof DomainError || failure instanceof BridgeError) && failure.outcome === 'not-started' && failure.code !== 'configuration-write-unknown';
        if(failure instanceof DomainError && failure.code==='shared-settings-write-permission-required')setWriteDenied(true);
        if (!rejected) { setUncertain(true); setError('Save outcome unknown. Refresh connections before making another change.'); }
        else setError(failure instanceof DomainError && failure.code==='configuration-changed' ? 'Settings changed in another window. Refresh connections and reopen the editor.' : errorText(failure));
      }
    } finally { if (alive.current) setBusy(false); }
  };
  const refresh = async () => { setRevision(v => v + 1); setBusy(true); setError(''); try { await read(); if(uncertain){setDraft(null);setRemoving(null);} setUncertain(false); } catch (failure) { setError(errorText(failure)); } finally { if (alive.current) setBusy(false); } };
  const start = (kind: Kind) => { setAdding(false); setDraft({ label: kind === 'ollama' ? 'Ollama' : 'OpenAI-compatible', template: kind, config: { baseUrl: kind === 'ollama' ? 'http://127.0.0.1:11434/v1' : '', model: '', requiresApiKey: kind !== 'ollama', maxOutputTokens: 1024, temperature: 0.7, thinking: false } }); };
  const selected = info?.connections?.find(c => c.id === info.defaultConnectionId);
  const requestAccess = async (connection?: Connection, config = connection?.config ?? draft?.config ?? selected?.config) => {
    if (!client.current || !config) return;
    const origin = originFor(config.baseUrl);
    const bound = connection ?? draft?.expected ?? selected;
    const secretRead = config.requiresApiKey && bound && bound.config.baseUrl === config.baseUrl && bound.config.requiresApiKey === config.requiresApiKey ? [keyPath(bound)] : [];
    try {
      await client.current.request('ui.request-permissions', { componentId: 'backend', runtime: ['http-request', ...(info?.worldId ? ['published-preference-read', 'published-preference-write'] : [])], httpOrigins: origin ? [origin] : [], secretRead });
      setNotice('Confirm access in Rintawa.');
    } catch (failure) { setError(errorText(failure)); }
  };
  const draftExpected = draft?.expected && draft.expected.config.baseUrl === draft.config.baseUrl && draft.expected.config.requiresApiKey === draft.config.requiresApiKey ? draft.expected : undefined;
  const draftProbe = useModels(domain.current, draft?.config, draftExpected, !!draft, revision, queue);
  const locked = busy || uncertain;
  return <main className="ai-settings"><header className="ai-heading"><h2>Ai Provider</h2><button className="icon-button" aria-label="Refresh connections" disabled={busy} onClick={() => void refresh()}>↻</button></header>
    <p className="ai-help">Settings are shared across worlds.</p>
    {busy && !info && <Status>Loading connections…</Status>}
    {info?.catalogError && <Status error>{info.catalogError === 'shared-connections-unavailable' ? 'Start Ai Provider on Home using the same installed package version.' : info.catalogError === 'shared-connections-permission-required' ? 'Allow access to the shared provider list for this World.' : info.catalogError}</Status>}
    {writeDenied && <Button disabled={locked} onClick={() => void client.current?.request('ui.request-permissions', {componentId:'backend',runtime:['published-preference-read','published-preference-write'],httpOrigins:[],secretRead:[]}).catch(failure=>setError(errorText(failure)))}>Allow shared settings…</Button>}
    {info?.catalogError === 'shared-connections-permission-required' && <Button disabled={locked} onClick={() => void client.current?.request('ui.request-permissions', { componentId: 'backend', runtime: ['published-preference-read'], httpOrigins: [], secretRead: [] }).catch(failure => setError(errorText(failure)))}>Allow shared providers…</Button>}
    {info && <div className="ai-toolbar"><Button className="primary" disabled={locked || (info.connections?.length ?? 0) >= 16} onClick={() => setAdding(true)}>Add provider</Button></div>}
    <div className="ai-connections">{info?.connections?.map(connection => <article className="ai-connection" key={connection.id}>
      <div className="ai-connection-name"><h3>{connection.label}</h3><ConnectionHealth domain={domain.current} connection={connection} enabled={!draft} revision={revision} queue={queue} />{info.defaultConnectionId === connection.id && <span className="ai-selected">Primary</span>}</div>
      <p>{connection.config.model || 'No model'} <span>· {connection.template === 'ollama' ? 'Ollama' : 'OpenAI-compatible'}</span></p><code>{connection.config.baseUrl}</code>
      <div className="ai-connection-actions">{info.defaultConnectionId !== connection.id && <Button disabled={locked} onClick={() => void mutate('set-default', { expected: connection, expectedDefault: info.defaultConnectionId })}>Make primary</Button>}<Button disabled={locked} onClick={() => setDraft({ expected: connection, label: connection.label, template: connection.template, config: { ...connection.config } })}>Edit</Button><Button disabled={locked} onClick={() => setRemoving(connection)}>Delete</Button><Button disabled={locked} onClick={() => void requestAccess(connection)}>Allow access…</Button></div>
      {connection.config.requiresApiKey && <details><summary>API key setup</summary><p>Store the API key in system credentials through your terminal, then choose Allow access:</p><pre>{`read -rs -p 'API key: ' rtw_api_key\nprintf '\\n'\nprintf '%s' "$rtw_api_key" | rintawa secret-set ${quote(keyPath(connection))}\nunset rtw_api_key`}</pre></details>}
    </article>)}</div>
    {info?.connections?.length === 0 && <Status>Add your first provider.</Status>}
    {info && !!info.connections?.length && !selected && <Status>Choose a primary provider.</Status>}
    {error && <Status error>{error}</Status>}{notice && <Status>{notice}</Status>}
    <Dialog open={adding} title="Add provider" onClose={() => setAdding(false)}><div className="ai-template-list"><Button onClick={() => start('ollama')}>Ollama</Button><Button onClick={() => start('openai-compatible')}>OpenAI-compatible</Button></div></Dialog>
    <Dialog open={!!draft} title={draft?.expected ? 'Edit provider' : 'New provider'} onClose={() => { if (!busy) setDraft(null); }}>
      {draft && <div className="ai-form" role="form" aria-label="Provider">
        <label htmlFor="ai-label">Name</label><Input id="ai-label" value={draft.label} disabled={locked} onChange={e => setDraft({ ...draft, label: e.target.value })} autoFocus />
        <label htmlFor="ai-url">API base URL</label><Input id="ai-url" value={draft.config.baseUrl} disabled={locked} placeholder="https://your-provider.example/v1" onChange={e => setDraft({ ...draft, config: { ...draft.config, baseUrl: e.target.value } })} />
        <div className="ai-model-heading"><label htmlFor="ai-model-choice">Model</label><ConnectionState probe={draftProbe} /><button className="icon-button" aria-label="Refresh models" disabled={draftProbe.state === 'Checking'} onClick={() => setRevision(v => v + 1)}>↻</button></div>
        <select id="ai-model-choice" aria-label="Available models" value={draftProbe.models.includes(draft.config.model) ? draft.config.model : ''} disabled={locked || !draftProbe.models.length} onChange={e => setDraft({ ...draft, config: { ...draft.config, model: e.target.value } })}><option value="">{draftProbe.models.length ? 'Select a model…' : 'No model list available'}</option>{draftProbe.models.map(model => <option key={model} value={model}>{model}</option>)}</select>
        <p className="ai-help">{draftProbe.detail}{draftProbe.omitted ? ` ${draftProbe.omitted} more models omitted.` : ''}</p>
        {['Permission required', 'Key permission required'].includes(draftProbe.state) && <Button disabled={locked} onClick={() => void requestAccess(draftExpected, draft.config)}>Allow access…</Button>}
        <label htmlFor="ai-model">Model ID</label><Input id="ai-model" placeholder="Select above or enter a model ID" value={draft.config.model} disabled={locked} onChange={e => setDraft({ ...draft, config: { ...draft.config, model: e.target.value } })} />
        <label htmlFor="ai-auth">Authentication</label><select id="ai-auth" value={draft.config.requiresApiKey ? 'bearer' : 'none'} disabled={locked} onChange={e => setDraft({ ...draft, config: { ...draft.config, requiresApiKey: e.target.value === 'bearer' } })}><option value="none">No API key (local Ollama)</option><option value="bearer">Bearer API key</option></select>
        <p className="ai-help">{draft.config.requiresApiKey ? 'The server requires an API key sent as Bearer authentication. Save this provider, then use API key setup to store the key in system credentials. Each scope needs its own read permission.' : 'Requests are sent without an API key. Use this for a local server that does not require authentication.'}</p>
        <details><summary>Advanced</summary>{draft.template === 'ollama' && <><label htmlFor="ai-thinking">Thinking</label><select id="ai-thinking" value={draft.config.thinking ? 'default' : 'off'} onChange={event => setDraft({...draft,config:{...draft.config,thinking:event.target.value==='default'}})}><option value="off">Off</option><option value="default">Server default</option></select><p className="ai-help">Off requests a visible answer directly. Server default may spend the output limit on reasoning.</p></>}<label htmlFor="ai-output">Maximum output tokens</label><Input id="ai-output" type="number" min={1} max={4096} value={draft.config.maxOutputTokens} onChange={e => setDraft({ ...draft, config: { ...draft.config, maxOutputTokens: Number(e.target.value) } })} /><label htmlFor="ai-temperature">Temperature</label><Input id="ai-temperature" type="number" min={0} max={2} step={0.1} value={draft.config.temperature} onChange={e => setDraft({ ...draft, config: { ...draft.config, temperature: Number(e.target.value) } })} /></details>
        {!!draft.expected && <Status>Changing the URL or key requirement needs approval for the new destination or credential.</Status>}
        {writeDenied && <Button disabled={locked} onClick={() => void client.current?.request('ui.request-permissions',{componentId:'backend',runtime:['published-preference-read','published-preference-write'],httpOrigins:[],secretRead:[]}).catch(failure=>setError(errorText(failure)))}>Allow shared settings…</Button>}
        {error && <Status error>{error}</Status>}
        <footer><Button disabled={busy} onClick={() => setDraft(null)}>Cancel</Button><Button className="primary" disabled={locked || !draft.label.trim() || !draft.config.baseUrl.trim() || !draft.config.model.trim()} onClick={() => void mutate(draft.expected ? 'edit-connection' : 'add-connection', { ...(draft.expected ? { expected: draft.expected } : {}), label: draft.label, template: draft.template, config: draft.config })}>Save provider</Button></footer>
      </div>}
    </Dialog>
    <Dialog open={!!removing} title="Delete provider" onClose={() => { if (!busy) setRemoving(null); }}><p>Delete {removing?.label}? Its API key remains in system credentials until you explicitly remove it.</p><footer><Button disabled={busy} onClick={() => setRemoving(null)}>Cancel</Button><Button disabled={locked} onClick={() => removing && void mutate('delete-connection', { expected: removing })}>Delete provider</Button></footer></Dialog>
  </main>;
}
createRoot(document.getElementById('root')!).render(<App />);
