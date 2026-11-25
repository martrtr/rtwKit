# RTW packages

The experimental runtime, Portable UI, projection and durable-job adapters have
been removed. Their P10 replacements are real packages, with local browser/protocol acceptance:

- `web-runtime`: ordinary WASM provider for `rintawa.runtime.web-bundle@2`.
- `web-ui`: React/Dockview shell, explicit World lifecycle and saved layouts.
- `dice`: independent Canvas/queryable panel demonstrating the browser SDK.
- `character`: World-local Tavern V2/V3 import, source CAS and portrait preview.
- `chat`: persistent conversations, independent listed forks, edit/delete and regeneration,
  with an ordinary WASM/System owner.
- `narrator`: bounded Character/history prompts and stale-context guarded assistant replies.
- `ai-provider`: non-secret World settings and bounded HTTP/SSE generation.

The web packages use the runtime provider; each has its own immutable RTW.
Reusable native/WASM libraries live under `crates/`. Browser authoring code lives
under `web/`; it is build-time code, rather than an extra installed service.

A new independently versioned extension belongs in `packages/<slug>/`. Its real
build must produce a valid RTW root before a release can be created. For example:

```toml
schema = 2

[package]
id = "rintawa.rtwkit.example"
name = "Example"
version = "0.1.0"
description = "Example rtwKit package"
license = "GPL-3.0-only"
authors = ["Example Author"]
readme = "README.md"

[build]
artifact-root = "build/rtw"
command = ["bash", "build.sh"]
```

Every direct child directory must be a complete package with `rtwkit.toml`.
Schema 1 and old package-ID replacement aliases are no longer supported.
Optional bounded `logo` and `readme` assets are published with HTTPS URL,
SHA-256, size and media type so a manager can display them without executing RTW.

The build command is an argv array executed in the package directory. Shell
syntax is not interpreted; invoke `bash` explicitly when needed. Production
installation must never execute source build scripts.

Tag `pkg-<slug>-v<version>` publishes stable asset names `<slug>.rtw` and
`<slug>.registry.json`. The tag, version and digest identify immutable bytes;
do not reuse the removed experimental releases' version coordinates.


Embedded surfaces use `launchable = false` and ordinary opaque declarative
`[[contributions]]` point/payload entries, not a settings flag. UI protocols and
bounded nested mounting are specified in docs/specs/p10-browser-bridge.md. A
JS-only contribution is published before its iframe opens; dynamic contributions
may still use the existing polled transport. Renderer selection uses the same
provided-contract mechanism as other extensions, without priority-based authority.


The new [local package manager](package-manager/README.md) is a separate optional
Host extension. It manages exact local archives and scoped selections using the
current APIs; it does not restore the retired experimental package manager.

Chat's message menu provides **Edit**, **Fork here**, **Regenerate** (after a user
input), and **Delete**. Edit creates a separate chat ending at the replacement.
A final user message offers **Generate reply**, including after editing or forking,
without adding an extra user turn. The delete dialog distinguishes removing one entry while keeping later messages
from discarding the continuation. Originals remain in the chat list. Regeneration
publishes its new prefix first; failure leaves it available for an explicit retry.
Stop saves validated received text into the selected generation's own chat.

Forks share immutable message text and copy conversation-specific entry relations
in one atomic transaction. Existing @1 data needs no migration. Large transforms
can fail with `history-too-large`; no partial chat is published. These are transcript
branches within one World, so they do not roll back simulation state or erase
historical copies. Message contributions and player personas are separate increments.


Chat also supports isolated message views via `rintawa/chat/message-views/v1`.
They open explicitly beneath a message, with at most four visible expansions,
and fetch their own data from captured entry IDs. Authors use `client.initialState`
for the invocation context; their own preferences are separate. The shared web
bridge clips panels inside the transcript and tracks resizing. Install the updated
Chat and web-ui together; details are in `docs/specs/p10-browser-bridge.md`.
