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
The selected conversation and saved messages survive Host restart. World-scoped
Narrator and AI Provider implement bounded OpenAI-compatible SSE generation; Chat
commits assistant replies only if the initiating conversation boundary is unchanged.
Deterministic loopback acceptance is documented below. Paid provider integration is
not claimed; native credential configuration uses stdin/system credentials and exact local
secret-read approvals, as specified in Core `docs/specs/host-secret-configuration.md`.
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

`Cargo.toml` pins Core artifact tooling. The browser/AI checkpoint runs against Core `1948004` plus its companion native
secret-configuration change, including the World-file transfer WIT. Template creation itself is available from Core `60a1597`. Before publishing, pin that committed Core revision together
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
for package in web-runtime web-ui settings-ui dice chat character narrator ai-provider; do
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
containing the built runtime, shell, settings-ui, ai-provider and TextRoleplay packages.
The runtime, shell, settings and shared AI catalog run in baseline; template installation never activates domain packages
or approves their permissions.
Open the private URL printed by Host. Choose **New World**, select **Text Roleplay**,
review its permissions, enter a name and choose **Create world**. The new World
starts and opens **Import a character**. Import Tavern V2/V3 PNG or JSON; the
workflow commits the Character, creates its linked Chat and opens it automatically.
Conversations stay in a collapsed drawer. A failed Chat creation preserves the
Character and offers **Continue to chat**; unknown mutations are reconciled by IDs
without automatic replay. World controls open a popup with confirmed deletion and stopped export. The create
selector accepts an installed template or **From file (.rtw)…**. Exported RTWs create
independent saved stories. The template includes Narrator and a World AI Provider.
Install the **same ai-provider.rtw** in baseline and allow `background-task` for its
`backend` and `settings` components, plus `http-request` for `backend`. On Home open **… → Settings → Ai Provider →
Add provider** and choose **Ollama** or **OpenAI-compatible**. Enter your model and
endpoint; the model list loads after this scope’s HTTP/key approvals. The first
provider becomes primary; **Make primary** changes it later. Every World uses the
current primary automatically. Settings in **… → Settings → Ai Provider** has the
same editor everywhere, and edits from a World change the shared settings for all
Worlds. Creation approves known access and shared-setting editing in the ordinary
permission list, with one checkbox. Additional missing rights apply immediately
without restart. The shell needs runtime-policy-write for that administrative action.
API keys use native stdin/system credentials, with exact read approval per consuming
backend; keys never enter the browser or export. Chat works without AI.

Character import saves the card greeting as the initial assistant message. The
source stays exact; card `{{char}}`/`{{user}}` markers expand once to the Character
name and the current default user label **You**. An oversized greeting keeps the
Character and offers explicit **Start without greeting**.

The catalog and primary are local installation preferences. Imported Worlds retain
saved messages/assets and use this installation’s primary after fresh permission review. Home and World AI RTW
digests must match; a missing publisher or different revision never falls back to
another provider. Settings lists only contributions running in its selected scope.
Templates do not automatically update existing stories. Browser fallback downloads are
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
Set `RTWKIT_AI_SMOKE=1` when running `story-onboarding-smoke.mjs` to also start a
loopback-only synthetic SSE server. The check covers saved replies and config,
HTTP errors, incomplete/oversize/timeout results, stale-context rejection, released
pending work, World stop and re-import without copied HTTP grants. No real provider,
private card, API key or paid request is used.

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

AI settings automatically checks the model list with backend-owned HTTP. Approve
`http-request` for the baseline backend when installing it and explicitly allow each
local origin in the grouped **Allow access** dialog. Live shell approvals take effect
immediately; offline CLI edits are loaded at activation. Choose a model from the list or enter its ID.
Online reports the last successful model-list check, not a generation guarantee.


Settings and Home are independently selected ordinary UI providers. settings-ui
supplies the default scoped Settings and Appearance; AI publishes its section over
Zenoh, and Dice publishes a Chat composer action before its view is opened. The
Root shell owns isolated viewports and administrative consent. No Better products
or Package Manager are required.

World creation includes the known AI origins/credential paths in its one consent;
using the current primary needs no per-World selection or repeated consent. Ollama defaults Thinking
Off (Advanced can use Server default); generation has 90 seconds while inventory
keeps the short timeout. The installed local Qwen model is covered by real browser
acceptance, not only the loopback fixture.

```sh
python3 scripts/build-ui-protocol-fixtures.py --artifacts /tmp/rtwkit-artifacts --output /tmp/ui-fixtures
RINTAWA_BINARY=/path/to/rintawa RTWKIT_SMOKE_ARTIFACTS=/tmp/rtwkit-artifacts RTWKIT_UI_FIXTURES=/tmp/ui-fixtures node scripts/ui-replacement-smoke.mjs
RINTAWA_BINARY=/path/to/rintawa RTWKIT_SMOKE_ARTIFACTS=/tmp/rtwkit-artifacts RTWKIT_OLLAMA_MODEL=your-installed-model node scripts/ui-protocols-smoke.mjs
```

The latter sends only a synthetic card/message to the explicit local Ollama server;
it never downloads a model or reads actual credentials. Omit RTWKIT_OLLAMA_MODEL to
use a deterministic loopback server. Existing Worlds keep their exact old RTWs;
the fresh-home helper installs current packages without rewriting those Worlds.
