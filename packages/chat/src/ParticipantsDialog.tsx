import { useState } from 'react';
import { Button, Dialog, Input, Status } from '@rintawa/ui';

export type Participant = { actorId: string; name: string };
export type ParticipantDraft = {
  conversationId: string; expectedConversationRevision: string;
  expectedLastEntryId: string | null; items: Participant[]; legacy: boolean;
};
type Props = {
  draft: ParticipantDraft; cards: { id: string; card: { name: string } }[];
  more: boolean; busy: boolean; unknown: boolean; error: string;
  onMore: () => void; onClose: () => void;
  onSave: (draft: ParticipantDraft) => Promise<void>;
};

export function ParticipantsDialog({ draft, cards, more, busy, unknown, error, onMore, onClose, onSave }: Props) {
  const [items, setItems] = useState(draft.items);
  const [choice, setChoice] = useState('');
  const available = cards.filter(card => !items.some(item => item.actorId === card.id));
  const bytes = new TextEncoder().encode(JSON.stringify({ items })).length;
  const limit = items.length > 64 || bytes > 8192;
  const invalidName = items.some(item => !item.name.trim() || new TextEncoder().encode(item.name).length > 256);
  return <Dialog open title="Participants" onClose={() => { if (!busy) onClose(); }}>
    <p className="dialog-description">Characters in this conversation. This list does not determine who is nearby or what each character knows.</p>
    {draft.legacy && <p className="dialog-description">Saving enables a shared conversation directed by Narrator. Group Psychology processing is not available yet.</p>}
    <ul className="participants-list" aria-label="Conversation participants">
      {items.map((item, index) => <li key={item.actorId}><label>Display name<Input aria-label={`Participant ${index + 1} name`} value={item.name} disabled={busy || unknown} onChange={event => { const name = event.target.value; setItems(current => current.map(row => row.actorId === item.actorId ? { ...row, name } : row)); }} /></label><Button aria-label={`Remove ${item.name}`} disabled={busy || unknown} onClick={() => setItems(current => current.filter(row => row.actorId !== item.actorId))}>Remove</Button></li>)}
    </ul>
    {!items.length && <p className="dialog-description">No characters selected. Player messages and narration do not require a character account.</p>}
    <div className="participants-add">
      <label htmlFor="participant-choice">Add character</label>
      <select id="participant-choice" value={choice} disabled={busy || unknown} onChange={event => setChoice(event.target.value)}>
        <option value="">Choose a character</option>
        {available.map(card => <option key={card.id} value={card.id}>{card.card.name}</option>)}
      </select>
      <Button disabled={busy || unknown || items.length >= 64 || !available.some(card => card.id === choice)} onClick={() => {
        const card = available.find(card => card.id === choice);
        if (card) { setItems(current => [...current, { actorId: card.id, name: card.card.name }]); setChoice(''); }
      }}>Add</Button>
      {more && <Button disabled={busy || unknown} onClick={onMore}>More characters</Button>}
    </div>
    <p className="dialog-description">{items.length} / 64 characters. Removing a participant preserves their earlier messages and memories.</p>
    {limit && <Status error>The participant list exceeds this build’s size limit. Remove a character before saving.</Status>}
    {invalidName && <Status error>Each display name must be nonempty and at most 256 UTF-8 bytes. Shorten the name here; the character card and earlier messages stay unchanged.</Status>}
    {error && <Status error>{error}</Status>}
    {unknown && <Status>The save outcome is unknown. Close this editor and use Reread World before another change.</Status>}
    <footer className="history-footer"><Button disabled={busy} onClick={onClose}>Cancel</Button><Button className="primary" disabled={busy || unknown || limit || invalidName} onClick={() => void onSave({ ...draft, items })}>Save participants</Button></footer>
  </Dialog>;
}
