# Psychology

Combined character memory and psychology extension. The product name is Psychology;
existing `rintawa.rtwkit.memory` and service/schema identities are retained.

Optional author inspector for explicit Character-card/conversation subscriptions.
Process one captured Chat page on demand. Literal observations preserve entry,
message, revision and role evidence; authored recollections have no source claim.
Narrator can receive bounded active recollections only after explicit opt-in on
the exact subscription. This does not infer that an actor witnessed the text.
Psychology belongs to this same package. The first explicitly authored fictional
model, `affect-v1`, tracks valence, activation and stress per exact subscription.
There is no separately required Psychology extension. Optional inferred affect and
recollection extraction run only after explicit opt-in.

Service `rintawa/memory/v1`, contract `rintawa.memory@1`. Every operation requires
this extension's exact World UI principal, except `narration-context` and its
owner-bound status/release, which also admit the selected Narrator backend.
The after-response notification admits only that selected Narrator backend.
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

## Authored affect

- get-affect `{subscriptionId}` returns `{subscriptionId,state}` (null before initialization);
  get-appraisal `{recordId}` returns `{recordId,appraisal}` (null before appraisal).
- begin-set-affect `{subscriptionId,expectedSubscriptionRevision,expectedStateRevision,enabled}`;
  expectedStateRevision is null only before initialization.
- begin-appraise `{subscriptionId,recordId,expectedStateRevision,expectedRecordRevision,elapsedSeconds,deltas}`.
- begin-advance-affect `{subscriptionId,expectedStateRevision,elapsedSeconds}`.

These methods are own-UI only. Mutations use the existing pending/status/release
protocol. Reconcile unknown outcomes by reading the known subscription's affect
revision and record's marker; never replay automatically. New enable/disable also
increments subscription revision, while appraisal/time advance change affect only.

Axes are integers: valence [-100,100], activation/stress [0,100]. Baseline is zero.
Appraisal deltas are integers [-20,20] per axis; elapsedSeconds is an integer
[0,86400] per command. Every crossed 600 accumulated simulation seconds moves
axes one point toward zero, before applying and clamping deltas. Reads, refresh
and narration never advance time. Total elapsed time and revisions are canonical
u64 strings with checked overflow.

Appraisal captures one exact active record revision in this subscription and
atomically saves state plus an immutable marker. Each record contributes once.
Editing/suppressing it does not undo or replay the past appraisal; a new authored
recollection can express a later correction. Disabling preserves values, time and
markers; ordinary Memory processing remains available. Other chat branches need
their own explicit subscription and have no implicit copy of this mutable state.

Both new properties (`rintawa.memory.affect@1`, `rintawa.memory.appraisal@1`) are
portable within the existing World store. Existing strict @1 schemas are unchanged.
Enabled affect is included in opted-in narration-context even with zero text
records, inside its existing 8 KiB bound. Narrator labels the fictional model and
axis ranges; the private lastRecordId is excluded. Disabling affect stops its
disclosure on future requests. No AI Provider changes or mechanical authority.

Desktop Pet will consume explicitly allowed presentation state and render avatars,
including Live2D inside the story/Web UI. Desktop overlays and real-time Companion
dynamics are deferred. Optional affect-cycles-v1 uses committed processing batches; authored affect-v1
still advances only through explicit time commands.

## Optional AI processing cycles

Psychology analysis is off until explicitly enabled per subscription. The panel
exposes enable/disable, messages per processing cycle (1–8, default 8), quiet cycles
before decay (1–100, default 3), and optional interpretation notes (4096 UTF-8 bytes).
These settings are World-owned and portable. Save settings while disabled does
not enable analysis. Enabling alone does not send an inference request.

- get-cycles `{subscriptionId}` returns `{subscriptionId,state}`; null means not configured.
- begin-configure-cycles `{subscriptionId,expectedSubscriptionRevision,expectedStateRevision,enabled,settings}`;
  settings is `{decayCycles,batchSize,interpretation}`; expectedStateRevision is
  explicitly null only before first configuration. Both revisions change on save.
- Existing begin-process automatically analyzes its next nonempty batch when this
  model is enabled. Process remains available explicitly; the separate optional
  after-reply switch below can also start one batch.

Processing sends bounded transcript text, observer name, prior cycle state and
interpretation notes to the configured AI provider, potentially incurring charges.
Connection/model selection uses existing AI settings. This consent is independent
of sharing recollections with Narrator. Disable stops future analysis and preserves
settings and values; basic literal processing then works without an inference call.

The model returns only per-axis deltas and reinforcement flags. Code validates
strict JSON, integer ranges and flag/delta consistency. Quiet counters increment
for unreinforced axes and move them one point toward zero at the configured
threshold. Reinforcement resets only that axis's quiet counter. One successfully
committed nonempty batch is one cycle. Empty reads, failures and retries do not
advance state; grouping a backlog differently may change its evolution.

State, literal observations and source checkpoint commit in one World transaction.
Invalid response, missing provider or revision conflict leaves all three untouched.
There is no fallback that silently consumes source after a failed analysis. A paid
inference can occur even when a later commit fails. Pending source remains available
for explicit retry or literal processing after disabling analysis. There is no
automatic retry of inference. Release before submission cancels analysis; release
after World submission cannot undo the transaction. Lost begin acknowledgements
may leave an unknown remote inference outcome; do not infer that no charge occurred.

New property `rintawa.memory.cycle-affect@1` and separate configuration/processing
commands preserve the older strict schemas. First cycle activation starts at zero
and atomically disables authored affect-v1; old values/markers remain stored.
Reactivation retains cycle state. Only one affect model may be active. Reducing a
threshold caps quiet counters below it without applying decay. Narrator can receive
the active cycle model only under the existing narration opt-in and byte budget.
No private actor data is automatically made available to avatar renderers.

## Processing after a saved reply

Separate default-off `automaticEnabled` switch on each subscription. UI provides
Enable/Disable after-reply processing and recent automatic status. AI analysis,
after-reply processing, subscription enable and Narrator sharing are independent.
If AI analysis is off, automatic processing only records literal observations.
Enabling alone does not start backlog work; one saved reply triggers at most one
bounded batch. Use Process to catch up larger backlogs explicitly.

- begin-set-automatic `{subscriptionId,expectedRevision,enabled}` uses the new
  `rintawa.memory.automatic-author@1` command, increments subscription revision
  and saves portable `rintawa.memory.automatic@1 {enabled}`. Missing means off.
- automatic-status `{subscriptionId}` is own-UI only and returns `{subscriptionId,
  enabled,targetMessages,report}`. Report is null when unavailable for this activation, pending
  `{entryId,status:"pending"}` or completed `{entryId,result:{status,reason?,itemIds?}}`.
  Only the last results for up to 16 recently completed pairs are retained in RAM.
- after-response `{observerId,conversationId,entryId}` is selected-Narrator-only.
  Returns scheduled or skipped, not a processing success or inference promise.

Narrator initiates one bounded notification only after a known successful append,
including a saved nonempty Stop prefix. The response's terminal status never waits
for notification acknowledgement or analysis. Unknown/failed/empty appends do not
notify. Selected Psychology processes through the notified entry, never a newer
concurrent reply, and uses existing atomic processing. Already-covered entries
are no-ops; one active automatic operation per observer/conversation pair prevents
parallel duplicate work. Busy notifications are skipped rather than queued.

Failures preserve committed Chat replies and source/affect state, are reported in
UI and logged with their reason. State-changing settings are revision-checked;
disabling while analysis is pending rejects its late commit. Input already sent
cannot be recalled and may have incurred a charge. Transient status/cleanup reads
may be retried within the operation deadline; generation begins are never retried.
Notifications/jobs are activation-owned, best effort, share the existing bounded
operation quota, and are not replayed after restart. Missing reports are unknown,
not successful processing. Automatic results refresh while the panel is visible;
closing/switching panels cancels read polling. Background state refresh preserves
unsaved psychology settings when persisted settings themselves have not changed.

### Automatic message target

The automatic panel exposes a new-message target (1–8, default 1). Saving it does
not enable processing or start a request. Automatic processing waits when its
complete source window has fewer unprocessed entries than the target. Its runtime
report has `result:{status:"waiting",available,target}`; waiting does not advance
checkpoint, create observations, change affect or call inference. Counts include
both speakers and an unprocessed greeting. The next saved reply reevaluates source.

A byte/row-capacity-bounded source window can run below the target so large messages
cannot starve processing. The target does not enlarge AI batchSize: a configured
smaller analysis batch still consumes only its prefix. Explicit Process always
bypasses this wait. This is a processing/spending setting, not a simulation clock.

`begin-set-automatic-policy {subscriptionId,expectedRevision,targetMessages}` uses
new `rintawa.memory.automatic-policy-author@1` and portable property
`rintawa.memory.automatic-policy@1 {targetMessages}`. Missing defaults to 1, preserving
existing behavior. Saves increment subscription revision and touch no flags,
checkpoint or affect. `automatic-status` exposes the effective target even while
processing is disabled. A recent waiting report records the target at that attempt;
changing settings or manually consuming source does not rewrite that old report.

## Extracted recollections

Independent default-off extraction settings: enabled, batchSize 1–8 (default 8),
maxRecords 1–8 (default 3), maxBodyBytes 128–2048 (default 1024), instructions up to
4096 UTF-8 bytes. Saving settings does not enable other features or trigger work.
These apply to future unconsumed source only; already processed history is not
backfilled in this slice. Existing automatic-target and source-cutoff rules apply.

- get-extraction `{subscriptionId}` returns `{subscriptionId,state}` where state is
  `{settings,recentIds}`. Missing storage returns disabled defaults without writes.
- begin-configure-extraction `{subscriptionId,expectedRevision,settings}`.
- list-records/get-record also return explicit kind `extracted`, authorEdited and
  provenance `{processor:"extract-v1",evidence:[...]}`. They retain their existing
  pending-read/status/release protocol and source check. Merged schema pages use
  the existing ID cursor. Evidence is immutable, nonempty and source-ordered.
- begin-update-recollection `{recordId,expectedRevision,body,status}` corrects or
  suppresses/restores an extracted record. Body limit is 4096 UTF-8 bytes, separate
  from the generator's per-record ceiling. Body changes permanently mark authorEdited;
  status-only changes preserve it. Source evidence never becomes human-authored.

Extraction-only performs one model request without enabling affect. If cycle affect
is also on, one joint request interprets the same window, capped by the smaller
batch size. Output is strictly validated; inference supplies only body/source-index
proposals and optional bounded affect deltas, never IDs, counters, writes or edits
to existing records. Exact duplicate body/source sets within one output coalesce;
no cross-window semantic deduplication or truth verification is claimed. Empty
recollections is valid. Combined generated bodies are capped at 8192 bytes and
response text at 16 KiB within the existing provider limit.

One transaction saves literal observations, optional affect, newly allocated
recollections, checkpoint and recent selection. Malformed output, stale settings,
source or state leaves the entire batch uncommitted. Basic processing remains
available by explicitly disabling enrichment; no fallback silently consumes source.
Releasing pending manual extraction cancels known inference before submission.
Generation begins are never automatically retried. Disabling keeps stored content.

New private properties `rintawa.memory.extraction@1` and
`rintawa.memory.recollection@1` and separate author/process commands leave existing
strict schemas intact. recentIds is a bounded persisted selection of at most eight
recent records, not an unbounded index or a second body store/checkpoint. All records
remain durable and browsable. Narration sharing independently permits selected
Narrator to resolve current revisions/status from this selection before the old
literal/authored fallback, inside the same eight-record/8-KiB budget. Records from
beyond captured source progress are excluded. Missing/malformed selection fails
explicitly. Current Narrator packages understand the new `extracted` descriptor;
update the companion packages together when enabling the feature. No compatibility
adapter for older strict consumers is introduced.

### Static registration schemas

Four extraction schemas are prepared natively by `examples/extraction_schemas.rs`.
`build.sh` refreshes checked-in `src/extraction_schemas.rs` before WASM compilation;
a native test compares each precomputed value with its authoritative Rust builder.
Fresh checkouts therefore compile without a missing generated input, and changes
to schema definitions are detected if the generated values are stale. Rebuild with
`bash packages/memory/build.sh` after changing these definitions. The Core's
registration fuel, memory, schema validation and permission limits are unchanged.
