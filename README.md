# rtwKit

rtwKit contains the extension packages and publishing tools for Rintawa. Product
policy belongs here; Core supplies sandboxed execution, exact activation identity,
scoped transport, permissions and atomic World storage.

## Current status

The experimental Portable UI/runtime stack has been retired. This checkout does
not yet provide a runnable Web UI, World Manager, Character importer, Chat or AI
runtime. Those packages must be implemented against the accepted D1–D8 contracts,
starting with the browser bridge and shell in P10. There are no publishable RTW
packages in `packages/` until their real implementations return.

Useful domain libraries are retained under `crates/`: Tavern JSON/PNG import and
portrait binding, CharacterTemplate RTW encoding, package catalog/dependency
solving, and inference codecs/SSE/model parsing. They perform no Host activation
or UI composition. See [cleanup scope and acceptance](docs/specs/p10-legacy-cleanup.md).
The machine-local `.vscode/` notes are historical and are not public requirements.

## Development

Use the toolchain in `rust-toolchain.toml` and run:

```sh
bash scripts/check.sh
```

`Cargo.toml` pins the exact Core revision; `wit/engine.wit` mirrors that revision's
public guest ABI. Do not introduce package-only Host imports or commit local path
dependencies. Editor/agent notes, local `.cargo/config.toml`, build output and
credentials are ignored. Dependencies can be checked offline when cached.

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
