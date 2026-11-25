# P10 browser foundation — implementation checkpoint

Status: browser foundation implemented with local acceptance; no release published.
This records implementation within the owner's accepted Rintawa browser contract,
`browser-layout-amendment.md` and `guest-monotonic-time-amendment.md`. It does not
approve new public guarantees. Core ABI is pinned to `256081cbfb2d608ca08362853fab452448768147`.

## Scope and ownership

`packages/web-runtime` is an ordinary root WASM execution-target provider for
`rintawa.runtime.web-bundle@2`. It owns loopback HTTP/WebSocket, session and channel
admission, static installed artifacts, bounded queues and cleanup. Native calls
run in the owning component's delegated callback; browser-supplied identifiers
cannot select execution context or acquire another channel's handles.

`web/bridge` provides the browser SDK, with MessagePort channels per sandboxed
iframe and a Root-only WebSocket client. Raw transport remains fanout; it does
not become a domain mutation router. Timeouts do not retry effects.

`packages/web-ui` provides the React/Dockview shell, explicit World lifecycle,
catalog, stable panel keys and shell-owned layout preferences. `packages/dice`
exercises independent panels and queryables. Shared React controls live in
`web/ui`. Chat, character import and package management are outside this checkpoint.

## Wire and lifecycle details

WebSocket responses carry the request's channel as well as its ID: IDs are scoped
per channel. The Root SDK supplies its own connection-wide IDs and strips channel
fields from MessagePort replies. MessagePort callers cannot choose authority.
The 96 KiB request limit applies to each WebSocket message, including when one
TCP read carries bytes from multiple messages. Each TCP read respects the
available input buffer capacity.

The runtime maintains at most one current single-use operator bootstrap token.
It can issue a replacement after consumption or expiry while session capacity
remains; existing session tokens are unaffected. Launch URLs are private and must
be redacted in test artifacts. Expiry uses Host monotonic time before credential
use and request dispatch; OS sleep may pause this clock.
If the clock is unavailable, authentication fails closed. A decoded request on
an authenticated connection receives a correlated `unavailable` error with
`outcome: not-started`. The runtime leaves the socket and its channels open.
Admission requires a fresh clock observation on every attempt. The caller
decides whether to retry.

Disconnect, unmount, logout, component stop and provider stop release their owned
channels and resources. Root reload authenticates the saved session token and
receives new channels. Layout persists exact World/instance/component identity,
never live channels, tokens or view URLs. Missing/ambiguous matches remain
placeholders. The pinned Dockview data is validated before `fromJSON`.

## Evidence and remaining acceptance

Native model tests cover HTTP/frame restrictions, layout bounds, typed calls,
session ownership and expiry. SDK tests cover correlation, limits, timeout without
replay and stable restoration. A real Host smoke has exercised D8 query/reply,
foreign-handle rejection, shell-only methods, layout, unmount and logout.

Reproducible local acceptance uses `scripts/browser-smoke.mjs` and
`scripts/native-bridge-smoke.py` with the actual three packaged RTWs and the pinned
Core CLI. Chromium checks real opaque iframe module loading, source-bound
MessagePort handshake, parent DOM/storage denial, two panels and D8 query/reply,
Canvas theme changes, layout reload, World switching, stop/reactivation, iframe
navigation with explicit reopen, absence of global postMessage RPC dispatch, and
full Host restart with stable instance-key restoration after explicit activation.
The protocol smoke checks exact Origin rejection, same-activation handle ownership,
admin denial, layout get/set/reset, unmount, fresh channels on same-token reconnect,
coalesced large requests, and logout across sibling connections while preserving
a distinct session. `python3 scripts/bridge-clock-regression.py` uses the same
`RINTAWA_BINARY` and `RTWKIT_SMOKE_ARTIFACTS` inputs. It builds a temporary runtime
with one failed authenticated clock read and verifies that the request has no
effect and both channels continue on the same connection.

Regression checks also cover asynchronous handler disposal, Dockview live serializer
optional properties, layout reference/parameter validation, session queue recovery,
and stale activation rejection. WIT errors use explicit case-to-code mapping;
binding-generator Debug strings are not protocol codes. The shell disables World
controls until initial layout/catalog reads complete.

Run `npm ci --ignore-scripts --no-audit --no-fund` then `bash scripts/check.sh`
for the repository gate. It includes native and WASM Clippy, Rust tests/docs,
repository metadata, TypeScript, browser unit tests, production builds and package
assembly/license notices. Browser smoke requires local loopback permissions and
installed Chromium. It uses a temporary browser profile and disables Chromium's OS
sandbox for the test environment; iframe sandbox and CSP remain enforced.

Character import, linked Chat and restart are implemented and exercised by
`character-smoke.mjs`. The current story delivery order is owned by Core
`docs/specs/mvp-story-delivery.md`; templates, workflow entry and AI remain later
slices. `native-bridge-smoke.py` also checks partial/coalesced large WebSocket
requests. `bridge-clock-regression.py` builds an isolated temporary runtime with
one injected authenticated admission clock failure, then verifies a correlated
not-started reply, unchanged layout and usable channels on the same socket. It
adds no production fault switch. Local acceptance does not establish coverage for
every hostile HTTP/frame combination, drag interaction or supported OS, and it
does not represent a release.

## Installed template and workflow connection (2026-11-23)

The owner-directed story delivery plan in Core records the exact checkpoint.
Shell-only methods are `world-sessions.list-templates({})`,
`world-sessions.template-info({digest})`,
`world-sessions.create-from-template({digest,title,approveRuntimePermissions})`,
and `world-sessions.entry({worldId})`. They reuse existing World session permissions
and native template validation/publishing. Creation never activates implicitly; shell
admits set-active and reconciles status before resolving the exact declared entry.
No title/package fallback and no automatic retry after an unknown creation outcome.
The catalog comes from installed RTWs. File transfer remains a later checkpoint.

All panels can use `ui.state.get({})` and `ui.state.set({state})`. State is one fixed
owner-scoped preference (`rintawa.ui.state.v1`), a flat string map with at most eight
entries, 64 ASCII bytes/key, 256 UTF-8 bytes/value, 1 KiB serialized. It carries
inert selection/recovery IDs; the consumer validates/reconciles them against World
data. No supplied preference keys, scopes, owners or runtime handles.

`ui.navigate({instanceId,componentId,state})` is local MessagePort navigation, not a
Native import or network dispatcher. RootBridge derives the source World from its
mounted binding, refreshes the live catalog and resolves exactly one target in that
same World. Missing/stale/ambiguous/foreign destinations fail. An admitted navigation
mounts the destination's own principal and passes inert state in rtw:init; it never
reuses the source's authority. Shell layout format remains unchanged. Current Chat
selection survives remount/restart through its own ui.state preference.

A web-bundle descriptor may declare at most eight `[[provides]]` rows containing
`id` and `version`. The runtime registers these as ordinary single-provider
contracts under the delegated component principal. The recipe explicitly selects
Character/story for rintawa.roleplay.workflow@1. Independent Character UI remains a
library when it is not the selected workflow. No Character method is built into
Core, the browser runtime or the shell.

## Administrative World files and menu (2026-11-23)

The owner-requested World menu now uses a native auto popover in the top layer.
It works over empty-World placeholders and opaque frames, focuses its first item,
closes on Escape/outside click/World switch and keeps existing workspace actions.
Delete captures one World ID and requires confirmation; export previews its data.
Both drain/stop that World before the Native operation; another World keeps running.
An unknown acknowledgement is reconciled by reads, never by replaying a mutation.

The exact Native transfer contract is in Core docs/specs/mvp-story-delivery.md.
Root-only browser methods are packages.begin-upload({size}), append-upload({handle,
offset,bytes}), finish-upload({handle}), status({handle}), read({handle,offset,
maximumBytes}), close({handle}), world-sessions.begin-export({worldId}). u64 values
are canonical decimal strings, chunks base64 and at most 32 KiB. Status is
{status:'uploading',written}, {status:'pending'}, {status:'installed',digest},
{status:'download',size}, or {status:'failed',reason}. No caller path/owner/scope.
Runtime keeps handles per channel; Native binds the exact activation and Host scope
plus world-session-write. Exact-owner close is allowed for cleanup after revocation.

At most two transfers/workers exist per process, including retained results and
abandoned workers until completion. RTW size is at most 1 GiB; Character limits are
unchanged. Staging uses private Host directories and existing live ownership locks
and crash recovery. Heavy fsync/validation/export runs outside guest callbacks.
Idle expiry is 120 seconds of Host time, checked on every use/callback cleanup;
OS sleep may suspend TTL. Close abandons accepted installation (which may finish),
never implicitly creates/activates a World and never approves requested grants.

From file (.rtw)… in the create selector opens a browser file picker. Cancelling
keeps the prior selection; corrupt/non-template input leaves no new World. A valid
installed digest goes through ordinary explicit approval and independent creation.
Export is prepared before the save picker/download is shown. File System Access
writes chunks directly; fallback Blob download is bounded to 64 MiB. Larger exports
need a streaming-capable browser or CLI. Known operations close on cancel/disconnect.

View catalog publication can follow activation. Shell briefly reconciles catalog
reads after observing a newly active World, preserving missing-view placeholders
and exact restore keys. Stopping Worlds close their frames before teardown. A
pending mount can be cancelled without inserting a late iframe; cleanup of an
already retired channel must not disconnect unrelated views. HTTP 403 for a racing
request to a retired bundle remains correct isolation, not a fallback to another
World's assets.


## Ordinary UI protocols (2026-11-23)

Core's generic sourced poll returns an attested active responder principal with
payload bytes; native/wire limits and one-shot query handles remain. `Client.querySourced`
uses that path. UI consumers validate their own versioned point and match the
response to the exact same-scope View instance/component/activation; payload owner
fields are not proof. Missing, duplicate and malformed routes are rejected with
an error state. No Root transport query in an arbitrarily supplied World.

A web-bundle may declare at most 16 opaque `{point,payload}` contributions totaling
64 KiB. The execution target publishes and polls them under that component's own
admitted task, so unopened JS-only views are discoverable. Native activation
teardown removes their routes. The target parses no Settings or Chat schema.
View metadata is `{id,title,worldId,restoreKey,activation,launchable,provides}`;
provided contracts report Native selection, and activation is a decimal string.
The old settings flag is rejected. All selected renderers are ordinary actors.

`rintawa.ui.settings.renderer@1` selects Settings; sections use
`rintawa/ui/settings/sections/v1` with describe -> `{id,title,order}`.
`rintawa.ui.home.renderer@1` separately selects full baseline Home content.
settings-ui supplies default Settings and Appearance; web-ui separates Root and
Home components. Explicit preferences resolve ambiguity; installation gives no
ambient administrative authority. Missing/stopped World renderers yield a
placeholder instead of borrowing baseline sections.

`Client.views` sees only its admitted scope. `Client.mountView` sends a same-scope
exact live reference and bounded viewport rectangle. Root creates the isolated
child as its own sibling iframe; consumers get no child channel, URL, nonce, port,
DOM or RPC authority. Rectangles are clipped to trusted visible overflow ancestors.
Closing a parent closes children; a Root-owned discovery timer handles external
retirement. Bound child calls revalidate ancestors. Idempotent close/clear works
without catalog admission. Unknown native mount acknowledgement disconnects that
socket to reclaim even unacknowledged channels. Missing bootstrap is released after
5 seconds. Pending close/reuse cannot delete a replacement's slot record.

Root retains its rail/workspace, privileged consent and narrow validated intents.
Selected Appearance may request a strict light/dark global theme change. Selected
baseline Home may ask Root to create/open a World; it inherits no Root capabilities.
World lifecycle reads through ordinary frames require their own Native capabilities.

Chat owns `rintawa/chat/composer-actions/v1`, describe -> `{id,label,order}`.
Dice declares the first self-view action; it is visible before Dice mounts, opens
Dice as its own actor and keeps Chat. Only a template entry is closed on workflow
navigation. No arbitrary URL, foreign DOM or delegated Chat writer.

Reproduce with build-ui-protocol-fixtures.py + ui-replacement-smoke.mjs; fixtures
only demonstrate alternate renderers and are not new Better products. Actual local
Ollama acceptance uses ui-protocols-smoke.mjs with an explicit installed model.


## Browser appearance and internal panes (2026-11-24)

Owner-selected UI slice; no Native API, World schema or transport changes.
The existing theme delivery supplies semantic tokens for text/surfaces, selection,
hover, focus, disabled states, feedback and accent foregrounds to every isolated
view. DockView uses the same tokens in both theme modes. Shared CSS and feature
CSS use these tokens; an authored image keeps its original colors.

`@rintawa/ui` supplies a local `SplitPane` for a sidebar and content. This is a
browser component, not a view/extension registry. Chat uses it for conversations.
The sidebar starts closed. Its existing toggle restores it; Enter on the focused
separator collapses it and returns focus to the toggle. Arrow keys resize (Shift
for a larger step), Home/End select bounds, double-click restores the default
ratio; pointer cancel/Escape discard a drag. Pointer capture is released when the
drag ends or the pane unmounts. No World command is issued by layout changes.

Chat stores `navigationRatio` and `navigationOpen` alongside `conversationId` in
its existing component-owned `ui.state` preferences. Ratio is finite, bounded
0–1. Rendered width is clamped to 160–420px and reserves 280px for content
when available; narrower viewports proportionally relax minima. A clamped render
does not rewrite the user's stored ratio. One coalescing preference writer keeps
selection and layout updates ordered without concurrent writes. Save failure is
visible; reopening does not promise a failed preference was stored. No automatic
retry of a failed write.

Acceptance: dark/light shell, Chat, dialog, Settings and isolated section;
readable text/selection/action/focus states; pointer and keyboard resize,
collapse/restore, cancelled drag, selection plus resize persistence, reload and
same-home process restart at a narrower viewport; retire a view during dragging
and keep another view usable. Existing transcript/send behavior must remain.


Modal layout invariants: opening a dialog/popover must not resize the workspace,
its document iframe, or the composer's viewport. Native dialogs use the browser
top layer; structural wrappers must not participate as extra shell grid items.
Settings' wrapper uses `display: contents` and retains its DOM selector scope.
Check geometry while dialogs are open, including nested permission confirmation,
not only after closing them. Resize handles keep a wide hit area, with their
visible stroke touching the sidebar edge without an inset gap.

## Freshness and directed message actions (2026-11-24)

`@rintawa/domain` exports `observeResource`: subscribe before reading, coalesce
bounded resource hints, serialize authoritative reads and reconcile visible views
periodically (initially 5 seconds), on focus/visibility return and after writes.
It preserves displayed data on read failure, aborts retired observations and
releases subscriptions. A successful read applies before its dirty follow-up;
continuous hints cannot starve reads. Visibility metadata is opt-in in the panel
handshake, so existing panels do not receive unsolicited protocol messages.
Baseline catalogs reconcile independently in each World; notifications grant no
cross-World access. Chat, Character and AI publish only after successful writes.

`Client.beginQueryTo` / `queryTo` expose Native `begin-query-to` through
`transport.begin-query-to` with `{point,payload,expected}`. `expected` is the
Native sourced principal `{instanceId,componentId,activation}`; activation stays
a canonical decimal u64 string. Scope and caller come from the mounted channel.
Native checks the exact point pin and live activation before delivery. A wrong
pin or retired recipient never receives the payload. Sourced replies must match
the expected principal; abort closes retained query handles. An unknown outcome
never triggers an automatic replay. This additive operation preserves ordinary
queries and view contributions. Chat's separate message-action discovery point
uses captured conversation/message/revision IDs and provider-owned authorization;
it grants neither Chat writes nor a different caller's native channel.


## Host tools and World chooser intent — owner-requested UI refinement

Native-attested launchable Host views appear beside Worlds in the Home app bar.
Root uses its existing isolated Surface mount, never a child URL/channel supplied
by another extension. Selecting another app unmounts the previous view; unavailable
apps return to the Home renderer. Settings remains a separate renderer.

A live launchable Host view can request `ui.create-world {template?:digest}` to
open the existing Root chooser. Root validates strict args and installed canonical
template identity. Creation, permissions and actual mutation still require the
existing Root UI submit and native checks. World views are denied. Existing Home
renderer no-argument creation remains valid; direct `ui.open-world` stays restricted
to the selected Home renderer. This is navigation, not delegation of Root rights.


## Message views and internal scroll clipping (F3a, 2026-11-25)

Chat discovers `rintawa/chat/message-views/v1` with `{"method":"describe"}`.
An ordinary UI contribution replies `{id,label,order}` through the existing native
sourced view mechanism. Chat resolves exactly the responder's admitted same-World
view and activation; the descriptor supplies no executable URL or parent channel.
Four explicitly expanded views may be open in visible rows. Discovery alone mounts
nothing. Context/revision/selection change, offscreen rows and provider retirement
release bindings; replacement does not reopen them automatically.

The widget reads five strings from `client.initialState`: `conversationId`,
`entryId`, `messageId`, `expectedRevision`, `conversationRevision`. These are inert
captured context, not permissions or persisted preferences. A widget reads text
explicitly under its own principal using Chat `get-message` with the first four
fields. Chat proves membership via the exact entry relation. The existing
`message-actions/v1` contract is unchanged and embedding grants no Chat write API.

`Client.mountView` retains its public signature. It supplies the full intended
rectangle plus optional `clip` on the Root-owned `ui.slot.mount/update` wire.
Both rectangles use finite local `x,y,width,height` within the existing bounds;
width/height are nonnegative. Clip is the intersection of viewport and clipping
DOM ancestors. Root intersects it with its own trusted bounds. The outer box clips
while the iframe keeps its intended dimensions and offset. Old callers omitting
clip remain accepted; update Chat and web-ui together for the new SDK protocol.

Ancestor sizing is observed so resizing one accessory also updates later ones.
Geometry updates coalesce per frame with at most one request in flight per slot.
Cleanup shares concurrent closes and retries only idempotent slot close on bounded
transient unavailable/limit-exceeded responses, without raising RPC quotas or
replaying any domain mutation. Late mount completion is released after close.
