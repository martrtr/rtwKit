# Memory

Optional author inspector for explicit Character-card/conversation subscriptions.
Process one captured Chat page on demand. Literal observations preserve entry,
message, revision and role evidence; authored recollections have no source claim.
Narrator can receive bounded active recollections only after explicit opt-in on
the exact subscription. This does not infer that an actor witnessed the text.
Psychology belongs to this same package; its numeric models are not implemented
yet. There is no separately required Psychology extension.

Service `rintawa/memory/v1`, contract `rintawa.memory@1`. Every operation requires
this extension's exact World UI principal, except `narration-context` and its
owner-bound status/release, which also admit the selected Narrator backend.
No actor-parameter public retrieval.

Methods (camelCase fields):

- allocate `{count}` (1–3); list-subscriptions `{after?}`;
  get-subscription `{subscriptionId}`.
- begin-create-subscription `{subscriptionId,observerId,conversationId}`;
  begin-set-enabled `{subscriptionId,expectedRevision,enabled}`;
  begin-process `{subscriptionId,expectedRevision}`.
- list-records `{subscriptionId,after?}`; get-record `{recordId}`.
- begin-create-authored `{subscriptionId,recordId,body}`;
  begin-update-record `{recordId,expectedRevision,body,status}` (`active|suppressed`).
- status/release `{operation}`, describe `{}`.

Mutations return existing pending operation shape and terminal committed/failed/
unknown. Process uses subscription ID as its known result identity; compare
revision/checkpoint to reconcile an unknown outcome, never replay blindly.
Record reads also return pending with `itemIds:[]`; poll status for
`{status:"ready",value:...}` or failed, then release. Reads validate the source
through directed Chat queries; unavailable source fails explicitly. Lists return
`{items,next}` with flat DTOs containing `id`; summaries omit full record body.

Memory owns subscription and record properties only. UUID references are private
JSON fields, no public actor-memory graph. Checkpoint and observations commit in
one World command; no inference, background scheduler or durable job queue.
Subscriptions do not follow edited/forked chats automatically. Suppressing a
record keeps progress and evidence and cannot trigger reprocessing of that range.

Narration opt-in: `begin-set-narration {subscriptionId,expectedRevision,enabled}`
uses its own `rintawa.memory.narration-author@1` command and increments the
subscription revision. Missing `narration@1` means off. Subscription DTOs expose
`narrationEnabled`. The UI explains that enabling sends eligible recollections to
the configured AI provider. Disable affects future captured requests.

`narration-context {observerId,conversationId}` returns a transient read operation.
Only own UI and the exact selected Narrator backend may request it. Status/release
remain bound to the caller. Only active records of an enabled, opted-in matching
subscription are disclosed, after checking the Chat source. Result includes IDs,
revisions, checkpoint and `records`, `truncated`, `omitted`. Selection is bounded
to eight records, 8 KiB serialized result and 64 scanned records. Oversized bodies
are omitted with explicit status, never silently truncated. These are subjective
recollections/authored beliefs, not objective rules or verified witness events.
