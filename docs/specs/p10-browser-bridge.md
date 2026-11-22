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

The runtime maintains at most one current single-use operator bootstrap token.
It can issue a replacement after consumption or expiry while session capacity
remains; existing session tokens are unaffected. Launch URLs are private and must
be redacted in test artifacts. Expiry uses Host monotonic time before credential
use and request dispatch; OS sleep may pause this clock.

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
and logout across sibling connections while preserving a distinct session.

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

Remaining product work includes Character import and the first persistent Chat
slice. Local acceptance does not establish coverage for every hostile HTTP/frame
combination, drag interaction or supported OS, and it does not represent a release.
