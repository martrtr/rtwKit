# P10 Character and Chat: first persistent domain slice

Status: accepted by the owner on 2026-11-22, including the clarified release,
PNG/import limits, monotonic Chat entry ordering and generic 16 MiB resource limit.
D1–D8 remain the accepted Core boundaries. Implementation is in progress.

## Scenario and boundaries

In an explicitly started World, select a Tavern/Character Card V2 or V3 JSON/PNG,
import a Character, open a conversation, append a user message, restart Host,
explicitly start the World and reread the Character, portrait and messages.
Chat must work without AI. No global Character library, migration, jobs, automatic
mutation replay or new Core/WIT capabilities are introduced.

Two ordinary RTWs, `character` and `chat`, each contain one native-WASM domain
component and one web UI component. The domain component owns its World schemas
and exports World System plus ordinary task/transport handling. Parsing/import and
Character authority share that component; there is no separate importer service.
A shared build-time helper may implement the demonstrated identical 16-operation
lifecycle for these two backends. It is not an installed runtime service.

## Persistent data

Names below are initial versioned public schemas. Host binds each name to its
exact approved instance, independently of these logical names.

| Owner / property | Item | Data |
| --- | --- | --- |
| Character / `rintawa.character.card@1` | Character object | display `name` derived from source, optional sanitized `portrait` AssetRef, required exact Tavern `source` AssetRef |
| Chat / `rintawa.chat.conversation@1` | Conversation object | nonempty `title`, optional `characterId`, `lastEntryId: UUID|null` |
| Chat / `rintawa.chat.message@1` | Message object | `role: user|assistant`, nonempty `text` |
| Chat / `rintawa.chat.entry@1` | relation Conversation → Message | empty object marking the relation as a Chat entry |

The original V2/V3 JSON or PNG is the canonical immutable card document in CAS.
All Tavern fields, versions and unknown extensions remain in that exact source;
we do not introduce a replacement character-card standard. World stores a small
reference record plus its validated display name (a derived display value, changed
only with a card replacement). It does not duplicate the whole card as a giant
World property. No CharacterTemplate RTW/global catalog is required. Other mods
can own independent properties on the Character object under D1.

Chat messages are bounded to 16 KiB UTF-8. The UI initially supports user messages;
assistant is a declared data role, not permission for arbitrary providers to append.
A single encoded message record must also fit 48 KiB, including JSON escaping,
so it can always fit one transport page. Reject over-limit text before submission
and recheck the invariant in System. Conversation titles are 1–256 UTF-8 bytes. No edits/deletions/branching or automatic
greeting insertion in this slice. Conversations and message objects are distinct
items; entries are real relations. Append creates Message, relation and properties
atomically in one command. System accepts an append only when `entryId` is
strictly greater than the conversation's `lastEntryId` (or it is null), comparing
UUID values in the same order as relation pagination. Updating `lastEntryId` is
part of that transaction. Otherwise return `stale-entry-id` before any effect.
UI allocates IDs immediately before sending; an explicit user action is required
to retry with fresh IDs. Thus a successful late write never lands behind an
already-issued cursor. Concurrent writers can receive this explicit rejection;
conflict-free multi-author append is not promised by this initial contract.
Reads page by relation ID with a maximum of 32 messages
and a total encoded transport reply of at most 64 KiB; return fewer rows plus the
exclusive continuation cursor when needed. No second persisted index or revision.

## Import bounds and staging

One file at a time is processed per Character backend instance. Other admitted
imports hold only parameters and exact resource refs, never copied file buffers.
The initial limits are 4 MiB for JSON/decoded card metadata, 16 MiB for source PNG,
maximum 4096 pixels on each side and at most 4,000,000 pixels overall. Zero-sized
images and APNG animation chunks are rejected. Check declared lengths, IHDR
sizes and limits before expensive processing; metadata decompression must stop
at the output-size bound.

Do not decode/reencode pixels just to strip card metadata. Validate PNG structure
and CRCs, then build the portrait from an explicit permitted chunk set (image,
palette/transparency and bounded color/physical metadata). This is not a claim of
full image correctness validation. Preserve exact original source bytes separately.
Release intermediate buffers between stages and retain the existing WASM memory
limit. An input below these maxima may still fail on memory exhaustion; failure
must not leave a partial Character in World.

Current Core source has an 8 MiB per-resource limit
(`crates/host/src/runtime.rs`, `MAX_USER_RESOURCE_BYTES`). To implement the 16 MiB
PNG path, this proposal also raises that generic limit to 16 MiB; the 32-resource
count and 32 MiB aggregate remain unchanged. WASM import/upload byte budgets
already allow 32 MiB. No WIT/API or extra streaming service is introduced.
Test the exact per-file and aggregate limits with the ordinary resource path.

Import creates the Character and its property in one transaction after CAS writes.
CAS leftovers on rejection are acceptable; do not promise cross-store rollback.
Portraits are metadata-stripped raster PNGs. Card fields are data: no prompt/HTML
execution, URL following, automatic remote downloads or automatic extra asset
imports. V3 fields and external asset references remain preserved in source bytes; this slice
does not claim to fetch/render every V3 asset URI scheme.

## Public domain wire operations

Each owner defines one single-provider contract (`rintawa.character@1` or
`rintawa.chat@1`) and one matching authoritative transport point
(`rintawa/character/v1` or `rintawa/chat/v1`). The domain SDK obtains selected
provider identity from existing runtime-context APIs in WASM; browser callers
require exactly one well-formed domain response and never fan out mutation retries.
Operator composition pins each transport point to the exact component using
existing `profile.authoritative_providers`; contract lookup alone does not install
that pin. Backend verifies selected-provider agreement before mutation admission.
Bootstrap/acceptance must exercise both existing selections and reject ambiguous
or mismatched configuration. No semantic router is added to the generic bridge.

JSON requests are tagged by `method`, deny unknown authority fields and stay below
64 KiB. IDs are canonical UUID strings; operation tokens are opaque random 32-byte
values, never persisted or logged. Transport identity is the immediate attested
caller, never a claimed field in JSON.

| Method | Request / result |
| --- | --- |
| `allocate` | Own UI only: allocate 1–3 Host-generated item IDs, with no rows or command admission. UI obtains these before mutation so an unknown outcome can be reconciled by known IDs. |
| Character `begin-import` | `{itemId, resource}`; resource is the exact user-selected ephemeral ref; returns `{pending, operation, itemIds}` after bounded application admission. |
| Chat `begin-create` | `{conversationId, title, characterId?}`; returns the same pending envelope. |
| Chat `begin-send` | `{conversationId, messageId, entryId, text}`; returns the same pending envelope. |
| `status` | `{operation}` → `{pending}` / `{committed,itemIds}` / `{failed,reason}` / `{unknown,itemIds}`. Only committed follows SQL COMMIT. |
| `release` | `{operation}` makes the result unobservable; pending work continues under its existing slot. See lifecycle below. |
| Character `discard-resource` | Own UI only: release an exact ephemeral resource ref after upload/admission failure; no operation slot or World mutation. |
| Character `list` / `get` | bounded card-reference records or one Character by ID, including its source/portrait refs; never a full large card in a query reply. |
| Chat `list` / `page` | bounded conversations, or message entries for one conversation with optional exclusive cursor. |

Read-only domain methods explicitly admit active callers in the same World.
Mutation/allocation require the owner's own instance's `ui` component. Status and
release bind to the initiating exact caller activation plus opaque operation
handle. Tokens do not grant unrelated authority. No cross-package mutation or AI
append permission is introduced before its concrete Narrator contract is specified.
Every mutation route applies the same authorization, including alternate methods.
System rechecks domain invariants against its pinned snapshot. Client IDs confer
no permission; conflicting existing item IDs are rejected atomically.

## Operation lifetime

Each backend has at most 16 admitted operations. `begin-*` takes a slot through
completion; terminal results keep that slot until explicit release or stop.
Releasing a pending operation discards its observable handle/result, but work
continues and the slot is held until completion. Releasing a terminal operation
frees its slot immediately. `status` after any release returns `invalid-operation`.
The same rule applies to imported preparation and already-submitted World commands;
repeated begin/release cannot bypass admission or queue/memory bounds.

Work advances in short existing task callbacks; do not spin or invoke network/LLM
inside System. The importer stages resource read, parse, portrait/source asset
upload and World submission, releasing acquired resources/buffers on error/stop.
Stop cancels unfinished import preparation with cleanup. An already-admitted
World command may still finish; reconcile its result by reading known item IDs.
Close/release is not cancellation. Resource refs that were never handed to an
accepted backend operation can be discarded explicitly; native ephemeral resource
count/byte limits still apply if a browser disappears before handing them off.
PNG/RTW bytes are never put in a transport query payload.

The shared SDK may retry `status` with the same live operation after reply loss;
it never repeats `begin-*` automatically. A lost begin reply or unavailable command
receiver means unknown outcome: reread known IDs and require explicit user action.
Unreleased terminal results are retained before replying, so a lost status
response can be queried again. Stop/restart invalidates operations and scoped native handles.

## Acceptance

1. V2 and V3 synthetic JSON/PNG, malformed/oversized cards and deterministic
   source-format export checks; user's local corpus is supplementary and private.
2. Real UI upload → Character commit → portrait Blob rendering after Host restart,
   with no dependency on the ephemeral resource surviving the restart.
3. Create conversation/send/page through actual browser SDK and real WASM/System,
   reread after restart; a rejected second change rolls back the full transaction.
4. Unknown outcomes never trigger automatic mutation replay. Retryable status,
   retained terminal results, released-pending status invalidation, 17th-operation
   rejection even after 16 pending releases, slot recovery on completion and stop.
5. Deliberately reverse append order: newer entry commits, older is rejected as
   `stale-entry-id` without partial rows. Page cursors never miss a successful late
   append. Test concurrent send/rejection and explicit retry with fresh IDs.
6. One active import with queued refs, metadata decompression bound, image-size
   and APNG rejection, memory pressure cleanup, raw source fidelity and byte quotas.
7. Own UI accepted; other instance/component and JSON caller spoof rejected. Two
   Worlds remain isolated. Existing generic bridge regressions remain passing.

## Implementation progress

Core `cef1d86f87cbef45c3b7e34df97be154b1f4a41d` implements the accepted
[resource transfer amendment](https://github.com/martrtr/rintawa/blob/cef1d86f87cbef45c3b7e34df97be154b1f4a41d/docs/specs/user-resource-transfer-amendment.md).
It adds `take-resource`, replacement-ref privacy and native exact-activation
cleanup. No guest privileges are opened during stop. The per-resource limit is
16 MiB, aggregate 32 MiB. Real Host/WASM tests cover World stop and task trap;
context tests cover revocation/regrant and stale activation races.

The Chat package now provides its actual WASM/System backend and React UI.
Create/append emit atomic World transactions; append advances `lastEntryId` and
rejects stale entry IDs. The shared Rust operation state retains released-pending
work and its slot. The browser domain SDK retries only status reads and never
replays begin. The browser smoke exercises COMMIT, late-ID rejection with no extra
message, reload, a second isolated World and complete Host restart.

Real Chat testing exposed loss of System rejection text at the Core guest poll
boundary. A focused Core fix preserves that reason within the existing 2048-byte
UTF-8 limit; runtime acceptance uses this fix in addition to the pinned ABI.
Advance the Core pin when that fix is committed. This changes no WIT signature.

PNG parsing now checks dimensions, static structure and CRC, rejects APNG, keeps
an explicit portrait chunk set and bounds metadata decompression. It does not
validate/decompress the pixel stream. The V2 exporter emits a valid V2 envelope
for V3 inputs; original source-version metadata is not mutated. The owner's local
19-card corpus has 17 accepted cards and two explicit dimension-limit rejections;
private card files are not fixtures in Git. Synthetic tests exercise V3.

Still pending: the Character backend and browser file-import UI, actual
`take-resource` use at domain admission, staged source/portrait CAS writes, and the
combined browser Character import/portrait/restart acceptance. The complete P10
Character/Chat scenario is not yet declared finished.

No schema or permission changes beyond the accepted contracts are assumed accepted.
