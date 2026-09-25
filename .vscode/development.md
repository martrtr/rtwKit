# Development and bootstrap flow

## Developer sources

Published packages are immutable `.rtw` artifacts. Local development uses
`rintawa-dev`, which may run an explicit build command, create an ephemeral RTW
snapshot, and watch/reload it through the same validation and runtime path used
by production.

Build commands are allowed only for an explicitly selected trusted development
source or in CI. Normal installation never executes package build scripts.

Mutable developer sources are development-only state. They must not silently
become reproducible world dependencies; a future freeze operation may persist a
specific built artifact.

## Extension Manager

The Extension Manager is an ordinary `rintawa.extension@1` package. It owns
repository lookup, versions, dependencies, download, confirmation, install,
update, and uninstall UX. It does not own local source build/watch mechanics.

A Desktop distribution may prebundle its RTW artifact, but activation still uses
the normal artifact and extension runtime path.

## One-click install

External intents such as `rintawa://rtwkit/install?...` are routed to an
installed handler. The Extension Manager interprets the request and must show
package, source, version, permissions, and dependencies before installation.
Core does not implement repository or package-manager semantics.

## Secrets and provider credentials

Real provider credentials are allowed in development, but they must never become
package source, package metadata, fixtures, logs, release assets, or committed local
configuration. Provider packages must obtain credentials through the generic
component-scoped Core secret capability.

The repository security gate uses pinned Gitleaks 8.30.1 with verified release
archive checksums. `scripts/check.sh` scans complete Git history, unstaged tracked
changes, and the staged pre-commit diff. Package release CI repeats the scan before
building an immutable RTW artifact. No secret baseline or broad allowlist is used.
If a real credential ever enters Git history, rotate/revoke it rather than merely
deleting the current file.

## Implementation order

1. **Done:** run the React Web UI through the external `web-runtime` RTW provider.
2. **Done:** provide the Package Manager as an ordinary Portable UI extension with
   repository browse/search, exact-artifact install/update, dependency solving,
   permission review, enable/disable/uninstall, and immutable release provenance.
3. **Done:** cover Package Manager self-update through the real WebSocket/Portable
   UI path, exact persisted digest verification, and restart in release CI.
4. **Current MVP slice:** finish the thin runtime adapters for World Manager and
   Character Library now that their domain/content code lives in this repository. Keep them
   ordinary packages over `world-sessions`, Portable UI, content-handler, asset, and world
   contracts; do not add feature-specific Core APIs.
5. Add Chat and AI Provider as independently versioned ordinary packages using the generic
   Core service contract and component-scoped secret capability.
6. Add install intents/packs only when their concrete UX is required; they are not
   prerequisites for the first Chat + AI Provider composition.

## Dev-mode invariant

"Run from a directory" means no manual persistent RTW packaging step. The dev
tool may create ephemeral RTW snapshots internally; Core keeps one canonical
artifact format and activation path.
