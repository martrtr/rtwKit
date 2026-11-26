import { useEffect, useState } from 'react';
import { Button } from '@rintawa/ui';
export type AutomaticStatus = { enabled: boolean; targetMessages: number; report: null | { entryId: string; status?: 'pending'; result?: { status: string; reason?: string; available?: number; target?: number } } };
export function AutomaticControls({ enabled, runtime, disabled, onSave, usesAI }: { enabled: boolean; runtime: AutomaticStatus | null; disabled: boolean; onSave: (enabled: boolean, target: number) => void; usesAI: boolean }) {
  const savedTarget = runtime?.targetMessages ?? 3;
  const savedMode = !enabled ? 'manual' : savedTarget === 1 ? 'reply' : 'custom';
  const [mode, setMode] = useState(savedMode);
  const [target, setTarget] = useState(String(savedTarget === 1 ? 3 : savedTarget));
  useEffect(() => { setMode(savedMode); setTarget(String(savedTarget === 1 ? 3 : savedTarget)); }, [savedMode, savedTarget]);
  const valid = /^\d+$/.test(target) && Number(target) >= 1 && Number(target) <= 8;
  const desiredTarget = mode === 'reply' ? 1 : mode === 'custom' ? Number(target) : savedTarget;
  const changed = (mode !== 'manual') !== enabled || desiredTarget !== savedTarget;
  return <section aria-label="Keep it up to date"><h2>Keep it up to date</h2>
    <fieldset className="memory-cadence" disabled={disabled}><legend className="sr-only">Update frequency</legend>
      <label><input type="radio" name="cadence" checked={mode === 'manual'} onChange={() => setMode('manual')}/>Manually</label>
      <label><input type="radio" name="cadence" checked={mode === 'reply'} onChange={() => setMode('reply')}/>After every reply</label>
      <label><input type="radio" name="cadence" checked={mode === 'custom'} onChange={() => setMode('custom')}/>After <input aria-label="Messages between updates" type="number" min={1} max={8} step={1} value={target} disabled={disabled || mode !== 'custom'} onChange={e => setTarget(e.target.value)}/> messages</label>
    </fieldset>
    {mode === 'custom' && <small>Choose 1–8 new messages, including either speaker and greetings. Updates run after a saved AI reply; a full source window may run sooner.</small>}
    {usesAI && <small>Updates use the configured AI for enabled memory extraction or mood analysis and may incur charges.</small>}
    {!usesAI && <small>Updates currently keep source observations only. Enable extraction or mood analysis in Advanced settings to derive memories or mood.</small>}
    <small>Update now processes a batch immediately. Enabling automatic updates does not process old messages immediately.</small>
    {changed && <Button className="primary" disabled={disabled || mode === 'custom' && !valid} onClick={() => onSave(mode !== 'manual', desiredTarget)}>Save update frequency</Button>}
  </section>;
}
