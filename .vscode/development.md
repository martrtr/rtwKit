# Development and bootstrap flow

## Developer sources

Normal users install immutable `.rtw` release artifacts. Developers should not
need to manually package a `.rtw` after every edit.

The Extension Manager owns developer-source UX: selecting a local package/project,
running its explicit build command when needed, watching it for changes, and
restarting the affected extension instance.

For the first implementation, keep Core on one canonical artifact path. A dev run
may materialize an ephemeral RTW snapshot internally, but the developer should not
have to create, version, publish, or manage that file. Production still uses the
same RTW validation and activation semantics.

Do not make arbitrary package build scripts part of normal installation. Build
commands are allowed only for an explicitly selected trusted development source
or in CI. Published packages arrive prebuilt.

Mutable developer sources must be visibly marked as development content. They
must not silently become authoritative dependencies of reproducible State Engine
worlds. A future "freeze snapshot" operation can turn current dev output into an
immutable stored artifact.

## Extension Manager bootstrap

The Extension Manager itself is an ordinary `rintawa.extension@1` package. A
Desktop distribution may prebundle its RTW artifact, but it must pass through the
same import/permission/runtime mechanisms as third-party extensions.
## One-click install

One-click installation must not force network/package semantics into Core.

A browser or OS may open a generic external intent such as:

```text
rintawa://rtwkit/install?...
```

Core only receives and routes the external URI/intent to an installed handler.
The rtwKit Extension Manager owns the meaning of `install`: source lookup,
network access, version/dependency resolution, confirmation UI, download, digest
verification, and the final activation request.

Deep links must open an install proposal, never silently download and execute code.
The user sees package, source, version, permissions, and dependencies before the
manager performs installation.

If the manager is absent, Core reports that no handler is available. A future
verified HTTPS app link may provide web fallback, but custom `rintawa://` links
are sufficient for the first implementation.

## Implementation order

Do not start with the full graphical Extension Manager. Its UI itself depends on a
working UI layer.

Recommended order:

1. Finish the small Core bridge from stored RTW `rintawa.extension@1` artifacts to
   Extension Engine activation.
2. Implement the Extension Manager backend/headless capabilities needed for a
   local development source and restart/reload flow.
3. Implement React Web UI as the first real package, developed through that path.
4. Add the Extension Manager's Portable UI surfaces once Web UI can render them.
5. Add remote rtwKit source installation/update UX, dependency solving, packs,
   deep-link install handling, and richer repository management incrementally.
## Dev-mode invariant

"Run from a directory" means no manual or persistent RTW packaging step for the
developer. It does not initially mean that Core needs a second directory-native
artifact format.

Prefer an ephemeral snapshot produced by the Extension Manager because this keeps
validation, identity, and activation behavior identical to production. Introduce
a directory-backed Core `ArtifactView` only if profiling proves snapshot creation
is a real iteration bottleneck or a required hot-reload capability cannot be
implemented cleanly above Core.
