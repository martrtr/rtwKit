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

1. Run the React Web UI as a real Web bundle component through `rintawa-dev`.
2. Add Extension Manager Portable UI surfaces once the Web layer is usable.
3. Add remote repository install/update, dependency solving, packs, and install
   intents incrementally.

## Dev-mode invariant

"Run from a directory" means no manual persistent RTW packaging step. The dev
tool may create ephemeral RTW snapshots internally; Core keeps one canonical
artifact format and activation path.
