# Narrator

Explicit-roster chats use `begin-group-generate` with the same conversation,
triggering user message and first candidate message/entry IDs as `begin-generate`.
The operation identifies the affected conversation (`itemIds: [conversationId]`),
not a promise that those candidate IDs were published. Legacy generation is
unchanged. Group direction chooses one next speaker/narration/end at a time,
with at most four saved beats/eight inference calls under the existing deadline.
Each beat is an ordinary revision-guarded attributed Chat append. Earlier beats
survive later failure; reread the conversation after an unknown outcome.

The director sees public history, roster and optional Atlas lore; an actor writer
gets its own card and public history, without pooled private cards or Psychology.
Group pending status omits text so the single-text client callback cannot expose
director JSON or confuse several speakers. Saved beats appear independently.
Stop never publishes director fragments; writer prefix retention uses the same
provider/append lifecycle as the legacy path. Group Psychology, scene presence,
private perception and assistant-head Continue remain separate slices. Current
group history is explicitly shared, not an actor-confidential observation source.

World-scoped story MVP component. See ../../docs/specs/p10-narrator-ai.md.

Optional selected World Atlas contributes authored lore. Before inference,
Narrator asks its exact selected principal for `select-lore` using the UTF-8-safe
16 KiB tail of the last eight captured Chat messages. Atlas owns relevance;
Narrator validates the bounded reply and labels it author-supplied context, not
instructions or proof of actor knowledge. Lore is captured once per generation.
No Atlas preserves ordinary generation; a selected failed/invalid Atlas fails
explicitly before inference. Existing prompt budgets and stale append guards
remain. Install the updated Atlas with this Narrator when selecting both.

Optional selected Psychology receives a best-effort after-response notification
only after the reply's append is known committed. At most 16 transient query
observers with a five-second deadline are held independently of generation results.
Notification/analysis never changes a saved reply's success or delays its terminal
result. Psychology owns subscription opt-in, processing and diagnostics; Narrator
has no psychological state, scheduler, notification retry or persistent job queue.
