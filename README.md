# rtwKit

rtwKit contains the extension packages and publishing tools for Rintawa. Product
policy belongs here; Core supplies sandboxed execution, exact activation identity,
scoped transport, permissions and atomic World storage.

## Current status

The experimental Portable UI/runtime stack has been retired. Its replacement is
the P10 browser foundation: a sandboxed WASM web runtime, a browser SDK, a
React/Dockview shell and an independent Dice panel. The browser foundation has
local acceptance coverage. Chat now stores conversations and messages through
its own WASM/System backend and survives Host restart. Character imports Tavern
V2/V3 JSON/PNG into World with exact source bytes and sanitized portraits in CAS.
The installed Text Roleplay template creates and starts an independent World, opens
Character onboarding, then creates and opens its linked Chat after import.
The selected conversation and saved messages survive Host restart. AI/Narrator
runtime integration remains pending.
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

`Cargo.toml` pins Core artifact tooling. The World-file checkpoint requires the companion Core changes for
world-sessions package transfers, currently pending owner commit. Template creation
itself is available from Core `60a1597`. Before publishing, pin that committed Core revision together
with its matching `wit/engine.wit`; the older artifact-tooling pin alone is insufficient
to run these packages. Do not introduce package-only Host imports or commit local path
dependencies. Editor/agent notes, local `.cargo/config.toml`, build output and
credentials are ignored. Dependencies can be checked offline when cached.

Build a package locally through the registry tool, for example:

```sh
cargo run --locked -p rtwkit-registry-tool -- build-package \
  . web-runtime /tmp/rtwkit-artifacts local/source pkg-web-runtime-v0.1.0 local-test
```

Build the runtime, shell and domain packages, then the standalone template:

```sh
for package in web-runtime web-ui chat character; do
  bash "packages/$package/build.sh"
  cargo run --locked -p rtwkit-registry-tool -- build-package \
    . "$package" /tmp/rtwkit-artifacts local/source "pkg-$package-v0.1.0" local-test
done
python3 scripts/build-roleplay-template.py --artifacts /tmp/rtwkit-artifacts \
  --output /tmp/rtwkit-artifacts/TextRoleplay.rtw
```

With the current companion Core CLI, launch a fresh Host home:

```sh
RINTAWA_BINARY=/path/to/rintawa python3 scripts/dev-ui.py --artifacts /tmp/rtwkit-artifacts
```

The helper accepts `--core` instead of `RINTAWA_BINARY` and `--home` for a
nonexistent or empty directory. It prints the new home and a restart command;
use that command to return to the same Worlds. It never installs into an existing
Host home. You may instead point `--artifacts` at an existing local directory
containing `web-runtime.rtw`, `web-ui.rtw` and `TextRoleplay.rtw`. Only the runtime
and shell run in baseline; template installation never activates domain packages
or approves their permissions.
Open the private URL printed by Host. Choose **New World**, select **Text Roleplay**,
review its permissions, enter a name and choose **Create world**. The new World
starts and opens **Import a character**. Import Tavern V2/V3 PNG or JSON; the
workflow commits the Character, creates its linked Chat and opens it automatically.
Conversations stay in a collapsed drawer. A failed Chat creation preserves the
Character and offers **Continue to chat**; unknown mutations are reconciled by IDs
without automatic replay. World controls open a popup with confirmed deletion and stopped export. The create
selector accepts an installed template or **From file (.rtw)…**. Exported RTWs create
independent saved stories. AI replies remain pending. Browser fallback downloads are
bounded to 64 MiB; larger files require a streaming file picker or CLI.

Build `dice` in the same way when needed. To run the isolated Host/Chromium smoke, set
`RINTAWA_BINARY` to the CLI built from the pinned Core revision and
`RTWKIT_SMOKE_ARTIFACTS` to that directory, then run
`node scripts/browser-smoke.mjs`. `python3 scripts/native-bridge-smoke.py`
checks protocol isolation and multi-session logout with the same environment.
`node scripts/chat-smoke.mjs` additionally checks Chat COMMIT, stale-entry
rejection, two-World isolation and restart using runtime/UI/Chat RTWs.
`node scripts/character-smoke.mjs` checks file import, exact source bytes, portrait,
linked Chat and restart using all four RTWs.
`node scripts/story-history-smoke.mjs` checks bounded recent/older pages, concurrent
append, the reply byte limit and actual browser transcript scroll preservation.
`python3 scripts/bridge-clock-regression.py` builds a temporary fault-injected
runtime to check transient clock failure without a production debug switch.
`node scripts/story-onboarding-smoke.mjs` exercises the actual installed-template
chooser, automatic start/welcome, import-to-Chat, explicit continuation, two-World
isolation, forbidden cross-World navigation, restart, popup dismissal, file picker
cancellation, export/import independence and confirmed deletion.
All tests install only into temporary homes and preserve redacted logs there. Chromium must be installed; the test requires
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
