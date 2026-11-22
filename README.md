# rtwKit

rtwKit contains the extension packages and publishing tools for Rintawa. Product
policy belongs here; Core supplies sandboxed execution, exact activation identity,
scoped transport, permissions and atomic World storage.

## Current status

The experimental Portable UI/runtime stack has been retired. Its replacement is
the P10 browser foundation: a sandboxed WASM web runtime, a browser SDK, a
React/Dockview shell and an independent Dice panel. The browser foundation has
local acceptance coverage. Chat now stores conversations and messages through
its own WASM/System backend and survives Host restart. Character import and AI
runtime integration remain in progress.
See [implementation and acceptance](docs/specs/p10-browser-bridge.md).

Useful domain libraries are retained under `crates/`: Tavern JSON/PNG import and
portrait binding, CharacterTemplate RTW encoding, package catalog/dependency
solving, and inference codecs/SSE/model parsing. They perform no Host activation
or UI composition. See [cleanup scope and acceptance](docs/specs/p10-legacy-cleanup.md).
The machine-local `.vscode/` notes are historical and are not public requirements.

## Development

Use the toolchain in `rust-toolchain.toml`, including `wasm32-unknown-unknown`,
and Node.js 26. Install the pinned browser dependencies, then run:

```sh
npm ci --ignore-scripts --no-audit --no-fund
bash scripts/check.sh
```

`Cargo.toml` pins the exact Core revision; `wit/engine.wit` mirrors that revision's
public guest ABI. Do not introduce package-only Host imports or commit local path
dependencies. Editor/agent notes, local `.cargo/config.toml`, build output and
credentials are ignored. Dependencies can be checked offline when cached.

Build a package locally through the registry tool, for example:

```sh
cargo run --locked -p rtwkit-registry-tool -- build-package \
  . web-runtime /tmp/rtwkit-artifacts local/source pkg-web-runtime-v0.1.0 local-test
```

Repeat for `web-ui`, `dice` and `chat`. To run the isolated Host/Chromium smoke, set
`RINTAWA_BINARY` to the CLI built from the pinned Core revision and
`RTWKIT_SMOKE_ARTIFACTS` to that directory, then run
`node scripts/browser-smoke.mjs`. `python3 scripts/native-bridge-smoke.py`
checks protocol isolation and multi-session logout with the same environment.
`node scripts/chat-smoke.mjs` additionally checks Chat COMMIT, stale-entry
rejection, two-World isolation and restart using runtime/UI/Chat RTWs.
All install only into temporary homes and
preserves redacted logs there. Chromium must be installed; the test requires
local loopback sockets and uses headless Chromium with its OS sandbox disabled
for test-environment compatibility. Application iframe sandbox/CSP remain enabled.

## Distribution

Preserve independently versioned immutable RTW assets and a generated Pages
registry. Package source lives under `packages/<slug>/`; a tag such as
`pkg-web-ui-v0.1.0` builds `<slug>.rtw` and `<slug>.registry.json` as GitHub Release
assets. The registry tool combines release metadata into `index.json`, including
artifact digests, sizes and dependencies. Installed bytes come from the verified
RTW rather than a live UI CDN.

The registry URL is `https://martrtr.github.io/rtwKit/index.json`. Release deletion
requires rebuilding its deployed index; changing source alone does not remove
stale entries from GitHub Pages. The registry workflow supports manual rebuilds
and release-deletion events.

The owner requested removal of the old GitHub Releases during this reset. Source
tags remain historical provenance; new incompatible packages must use new version
coordinates. Future release and Pages workflows remain available. GitHub Actions
artifacts are diagnostics, not a package source.

See [package authoring](packages/README.md) and [registry output](registry/README.md).
