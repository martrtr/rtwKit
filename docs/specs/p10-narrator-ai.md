# Story MVP: Narrator, shared AI connections and settings

Implementation contract, 2026-11-23. The owner requested common connections and
conditioned acceptance on a universal extension mechanism. Core supplies generic
published preferences; AI-specific profiles and inference stay entirely in rtwKit.
See companion Core `docs/specs/extension-settings-and-ai-connections.md` for the
source-grounded design, alternatives, exact digest policy and local export change.
This supersedes this working slice's previous single World config/Chat dialog for
new exact RTWs. Existing immutable RTWs/Worlds are not migrated or rewritten.

## Scope and authority

The optional AI RTW contains an ordinary WASM backend and `settings` web component.
Its opaque declarative response on `rintawa/ui/settings/sections/v1` contributes
its own live view to the selected Settings renderer in its running scope.
`launchable=false` keeps it out of ordinary app launchers. `settings=true` is retired. Home and World expose the same shared catalog editor and primary selection. Worlds
automatically use the current primary. Appearance is an ordinary settings-ui contribution; its selected actor sends a
bounded theme intent to the Root owner for the same global theme in either scope.
Settings does not activate extensions or Worlds; it uses the existing iframe mount,
exact restore key, channel ownership, theme and cleanup. No Core UI registry.

Backend contract/point remain `rintawa.inference.text@1` / `rintawa/inference/v1`.
Only the exact sibling settings principal may administer connections and primary selection.
Only a World's exact selected Narrator may begin inference. Baseline never admits
inference. Descriptions to other callers expose neutral readiness/reason only.
The backend registers no World property/System in this new revision. The catalog and primary live in one declared baseline preference, without World
copies, bindings, sidecars or a new table.

## Shared configuration

The backend manifest publishes `rintawa.ai.connections.v1`. Core requires a live
ordinary baseline component of the same exact RTW digest and component ID, and an
explicit `published-preference-read` grant on each ordinary World backend. It
returns only that one preference, max64 KiB. All other keys remain private. Root
identity is derived from live verified loader bindings, not mutable profile choices.
Absent/stopped/ambiguous/mismatched sources reject the read. This is not cross-scope
transport, and no browser receives the shared-read import.

Catalog: `{connections:[{id,label,template,config}],defaultConnectionId:UUID|null}`, at most16 connections with
unique backend-generated random128-bit UUIDs; encoded catalog <=60 KiB reserves
transport space for settings metadata; label nonblank <=128 UTF-8 bytes.
Template is `ollama` or `openai-compatible`. Config remains
`{baseUrl,model,requiresApiKey,maxOutputTokens,temperature}`: safe HTTPS or separately
approved loopback HTTP; no URL credentials/query/fragment/control/backslash; model
nonblank <=160 bytes; output1..4096; temperature0..2. No implicit remote endpoint,
model download or generation when reading/saving settings. Automatic diagnostic
GET /models is described below, under separate scope-owned HTTP/key permissions.

Settings wire uses existing strict JSON envelopes and64 KiB transport:

- `describe`: neutral `{ready,reason}`; the sibling settings component additionally
  receives `{worldId,instanceId,connections,catalogError,defaultConnectionId}`. It never reads a
  secret merely to describe readiness.
- `add-connection {label,template,config}` creates a fresh ID atomically.
- `edit-connection {expected,label,template,config}` / `delete-connection
  {expected}` require the complete previous connection. Concurrent stale edits
  reject `configuration-changed`. Endpoint/key-mode changes generate a fresh ID;
  label/model/sampling edits preserve identity. Delete never reuses an ID on purpose
  or implicitly removes a key. Native secret-delete/revoke remains explicit.
- `set-default {expected,expectedDefault}` checks the displayed connection and
  previous primary, then changes the shared primary. The first added provider is
  primary automatically. Deleting a primary clears it without choosing another.
- All edits are shared from either scope. Home and World CAS the same declared
  published preference against the exact raw value loaded. World writes need the
  separate published-preference-write capability in addition to shared read.
  Native attestation, admission, digest fence and bounded atomic persistence follow
  the companion Core specification. A conflict preserves current state; an unknown
  save requires Refresh before another change. No write or generation is replayed.

Generation captures the current primary model/config at admission. Later edits affect
future requests in every World. Endpoint/key-mode edits rotate connection identity;
native HTTP/origin/key permissions remain independently enforced in each scope.

## Credentials and local HTTP

An exact connection derives `ai.connection_<32 lowercase hex digits>.api_key`.
Store through the accepted native stdin/keyring CLI once per connection. Each
consuming World needs its own exact secret-read approval; key values never appear
in preferences, UI state or export. Baseline discovery requires its own HTTP and, when configured, exact key-read approval.

Ollama starts with `http://127.0.0.1:11434/v1`, no key, and an entered model.
Settings offers the bounded intent `ui.request-permissions
{componentId,runtime,httpOrigins,secretRead}`. RootBridge resolves the current
requesting view and infers its own exact instance/scope. Extra authority fields
are rejected. It acknowledges showing consent without holding a panel RPC over
the user's decision. Root captures the attested artifact/activation and applies
only missing selected rights through root-only `runtime-policy.live-component`
and `runtime-policy.apply-live-change`, under runtime-policy-write. Ordinary
frames cannot inspect or apply native policy. Success means saved and live; the
backend, Settings frame and drafts survive without Host/World restart. Failed or
uncertain acknowledgements are reconciled by reading current access, not replay.
Exact secret-read is independently selected; native secret writes remain CLI-only.
The companion Core `docs/specs/live-permissions-and-ui-contracts.md` defines races,
revocation, cleanup and offline CLI semantics. Denied shared-catalog access has a
live Allow shared providers action, without requiring a configured connection.

## Narrator and commits

Chat owns all message commits. It saves a user message before invoking Narrator
with known assistant IDs, and offers explicit Retry reply after a failed generation.
Narrator accepts only the exact selected Chat UI. It reads one recent page and the
Character's original CAS source through its own granted reader, validates the current
user boundary, and builds a labelled prompt from name/description/personality/
scenario/system prompt/post-history instructions/dialogue examples. Only exact
`{{char}}`/`{{user}}` card markers expand once to the name/default display label You;
user-written messages and all other macros remain literal. There are no tools,
lorebook fetches, executable macros or persistent user principals.

Card plus initiating user message fit24 KiB content; history is newest chronological
suffix <=32 messages/16 KiB using remaining budget. Drop oldest whole messages only;
no silent card/current-message truncation. Encoded request <=60 KiB. CAS reads are
staged in32 KiB chunks under existing source/memory/fuel limits.

Provider `begin-generate {messages}` remains polled with16 backend slots including
retained terminal results. Status is pending/completed{text}/failed{reason}; release
is caller-activation-owned. Released pending work retains its slot/work until terminal.
HTTP is async POST /chat/completions with SSE, max4*8 KiB events/op/task, wire<=256 KiB,
answer<=16 KiB and encoded<=60 KiB. Require HTTP200, EOF/[DONE]/stop and nonempty
text; reject tools, malformed/incomplete/oversized streams and upstream error bodies.
The accepted streaming amendment in companion Core `docs/specs/ai-multiple-providers-streaming.md`
governs cumulative pending text and guarded interrupted-prefix retention. World stop retires only its resources;
external cancellation is best effort and already sent work can still incur cost.

Narrator asks Chat `begin-assistant-append` at the captured lastEntryId and user ID.
Chat System checks current boundary, exact relation/tag/role, distinct unoccupied
IDs, monotonic new entry ID and text bounds before one atomic message/relation/
boundary commit. Concurrent append yields stale-context. No SQL transaction spans
HTTP/transport, no restart replay, no durable generation job. Known IDs support read
reconciliation after unknown admission; absent data does not establish rollback.

Character import also writes its bounded derived greeting property in the same
transaction as its card reference. Chat creation can atomically save first_mes as
an initial assistant and lastEntryId. Empty greetings create an empty Chat;
oversize retains Character, reports refusal and offers explicit Start without
greeting. Original CAS bytes stay exact. Restart never repeats the greeting.

## Export and validation

New exports preserve graph, source/portrait, greeting and saved replies. The local
catalog, selection, endpoints/models, grants, key values and unfinished work do not
travel. Same-home forks use the local baseline primary after fresh consent; a clean
home without baseline AI remains unavailable. Manual Chat/lifecycle/export still work. This is the accepted
shared-configuration amendment to the previous portable-config result, not a
migration of old exact RTWs.

Validation uses local Native/WASM tests plus real Chromium and a loopback-only SSE
fixture with synthetic Tavern V2/V3 cards. Cover common catalog/edit, automatic shared primary, global edits from Worlds, explicit origin approval, sibling/caller denial, no dispatch on settings,
greeting-once, timeout/HTTP/SSE/oversize, stale context, released work, concurrent
Worlds and cleanup, restart/no replay, local-primary/clean-home export/import. In-memory vault
checks cover credential values; real credential/paid-provider access is not claimed.

## Model discovery and settings navigation — accepted 2026-11-23

The settings entry is **Ai Provider**. A settings-sibling-only
`begin-models {config,expected?}` captures a canonical destination and starts a bounded
GET /models. Config may have an empty model for discovery. Keyless baseline drafts
can be checked before saving. Credential-requiring drafts must identify a current
saved connection with the same destination/key mode; its exact secret path and local
read grant are required. A World checks only its selected current connection. No
other caller may start a probe. Baseline diagnosis does not enable generation.

The existing status/release handles and 16 retained slots are reused. At most one
probe is active per settings activation. Explicit release closes read-only probes;
generation retains its existing work semantics. Native HTTP deadline, stop and
revocation cleanup remain authoritative; unexpected frame loss cannot prolong the
request beyond that deadline. UI serializes probes, suppresses stale editor results
and requests release on replacement/close. No polling monitor or persisted health.

Reuse the existing ModelProbe decoder: 128 KiB /512 input records /48 sorted unique
model choices, bounded160-byte IDs, no upstream error bodies. Online describes the
last valid inventory response, including an empty one; Offline is transport/timeout.
Permission/key/authentication/incompatible-response failures are distinct. Manual
model ID entry remains available. A select updates the editor and Save persists it.
Opening Settings may now check models, but never generate. Readiness describe still
does not read a key; the explicit diagnostic operation may read one under its grant.

Baseline installation needs background-task + http-request for the backend; each
loopback origin is independently approved in the grouped captured confirmation.
The live dialog can grant HTTP and the exact origin together without restart. API keys remain native CLI credentials with exact per-scope approval.
UI explains authentication as None versus Bearer API key; no key enters the iframe.

Shell Settings is inside the ellipsis action menu on Home and in Worlds. Its sidebar contains sections only;
Appearance controls one existing global theme on Home and in every World. Extension
contributions remain strictly scoped. Workspace actions no longer duplicate theme.

Chat Send saves the user message before one automatic generation attempt. It reads
Narrator availability again if the startup check was not ready. Failed generation
shows explicit Retry reply; normal Send requires no separate generation button.
Unknown write/generation outcomes, failed saves, opening/reloading a transcript and
World restart never trigger automatic replay.


## Upfront consent and real Ollama correction

At World creation, the existing single consent includes known AI origins and exact
credential paths advertised by the baseline backend. Native creation validates
immutable slot/component/artifact and manifest ceilings, and saves grants under
fresh World IDs before publication. No source grant is inherited. The World uses
the local baseline primary automatically; existing approved access needs no second
popup. Rights share the ordinary creation list; there is no separate AI access block. A new destination/key later needs a delta approval; exports omit local grants.
Creation metadata is served only to the exact selected administrative shell principal.

Generation uses generic `http-request.start-with-timeout` with 90 seconds, within
Narrator's 120-second workflow. Model discovery keeps the short Host default.
Existing `http-request.start` behavior and limits are unchanged. One absolute
request deadline includes worker dispatch and is rechecked after DNS before send.
Blocking OS work may outlast logical cancellation, but no cancelled data is exposed.

Ollama defaults Thinking to Off, encoded as `reasoning_effort:"none"` on its
OpenAI-compatible endpoint; Advanced allows Server default. Other provider
protocols are unchanged. The installed qwen3.5:4b-q4_K_M consumed 1024 tokens in
reasoning for 96 seconds without visible text under its server default. A synthetic
full real Chat/Narrator/Provider run with Off saved its answer in 10.8 seconds.
These observations concern the installed local Ollama/model, not every server.


## Explicit Chat Stop — requested 2026-11-25

Chat displays Stop after Narrator admits a generation, including before the first
character. Clicking it sends `stop {operation}` to the exact Narrator operation,
then continues observing that operation until its real terminal result. The control
shows Stopping while this is pending. Closing/unmounting the view still detaches
observation/releases; it does not issue Stop or start a new retention command.

Narrator checks the original caller instance/component/activation using existing
operation ownership. Stop returns the existing terminal result if already finished;
otherwise it freezes the last validated Narrator prefix and returns pending.
Repeated Stop is idempotent while retained. No client-supplied text is accepted.
An in-flight inference begin is reconciled to obtain its handle before cancellation;
an admitted Chat append is reconciled rather than resubmitted or retracted.
Unknown begin/write outcomes never cause automatic generation or append replay.

AI Provider adds original-owner `cancel {operation}`. It closes the owned HTTP
handle and retains terminal `{status:"failed",reason:"cancelled"}` until release.
Cancelling terminal work returns its existing result. Provider owns no Chat write.
Narrator does not ingest new text from cancellation/status after accepting Stop.
It releases the provider observer and submits its frozen nonblank prefix through
ordinary Chat assistant append, with the original future IDs and captured revisions.
Only Chat COMMIT makes the saved message canonical. Stale context remains rejected;
empty Stop creates no assistant and the UI shows no interruption error. Invalid
snapshots and previously determined failures are not converted to success by Stop.

The native HTTP worker may remain blocked on a silent read until data/timeout.
The retained text can commit without waiting for that socket, and the native worker
continues holding its global worker permit until it exits. Stop does not promise
immediate upstream termination or cancelled billing. A stopped World/activation
cannot initiate a retention write. Committed history survives restart with no
replay of unfinished operations. No storage schema or Core API change is required.

Ship Chat, Narrator and AI Provider artifacts together; baseline and World Provider
must still use the same exact bytes for shared catalog admission. Existing exact
Worlds keep their installed contract until explicitly switched.

Acceptance extends `scripts/chat-streaming-smoke.mjs`: prefix retention before
waking a silent upstream, ignored late bytes, empty Stop, foreign caller rejection,
repeated Stop and stale Chat revision. The original stream/failure/restart checks
remain part of that same real Chromium/WASM fixture.
