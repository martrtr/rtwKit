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

## Implementation order

1. **Done:** run the React Web UI through the external `web-runtime` RTW provider.
2. **Done:** provide the Package Manager as an ordinary Portable UI extension with
   repository browse/search, exact-artifact install/update, dependency solving,
   permission review, enable/disable/uninstall, and immutable release provenance.
3. **Done:** cover Package Manager self-update through the real WebSocket/Portable
   UI path, exact persisted digest verification, and restart in release CI.
4. **Next MVP slice:** add Chat and AI Provider as independently versioned ordinary
   packages using the generic Core service contract and component-scoped secret
   capability. Do not add Chat/AI-specific host APIs or Core semantics.
5. Add install intents/packs only when their concrete UX is required; they are not
   prerequisites for the first Chat + AI Provider composition.

## Dev-mode invariant

"Run from a directory" means no manual persistent RTW packaging step. The dev
tool may create ephemeral RTW snapshots internally; Core keeps one canonical
artifact format and activation path.
