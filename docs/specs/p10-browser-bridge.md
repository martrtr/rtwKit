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
