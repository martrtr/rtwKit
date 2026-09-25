# rtwKit

rtwKit is the recommended package set and package source for Rintawa. It is not
part of Rintawa Core: every package is built and distributed through the same RTW
artifact model available to third-party repositories.

## Distribution model

Source code lives together in this monorepo, but users never need to clone it.
Each package version is published independently as one immutable `.rtw` GitHub
Release asset. A small GitHub Pages `index.json` lists the available versions, package
presentation assets and their SHA-256 digests. The root `registry.toml`
describes repository identity; the registry builder publishes its bounded icon
next to the index and emits a verified asset descriptor.

```text
packages/<slug>/ source
        ↓
pkg-<slug>-v<version> tag
        ↓
GitHub Release: <slug>.rtw
        ↓
GitHub Pages: index.json
```

Asset names are stable across versions. The release tag and SHA-256 digest carry
the versioned/immutable identity, so `pkg-web-ui-v0.0.1` and
`pkg-web-ui-v0.2.0` both contain an asset named `web-ui.rtw`.

The intended Extension Manager source URL is:

```text
https://martrtr.github.io/rtwKit/index.json
```

GitHub Actions workflow artifacts are used only for CI diagnostics and are not
part of the public package source.

## Current packages

- `package-manager` — repository-backed extension discovery, dependency solving,
  permission review, and exact-artifact activation.
- `web-runtime` — external `rintawa.runtime.web-bundle@1` execution-target provider.
- `web-ui` — React Host Shell / Portable UI Layer executed through `web-runtime`.
- `world-manager` — standard replaceable World catalog/lifecycle Portable UI package.
- `character-library` — CharacterTemplate content handler plus read-only Library
  Portable UI over the generic user-content capability.

Published `(package id, version)` coordinates are immutable: a released version is
never rebuilt or replaced in-place. The registry digest identifies and verifies the
exact artifact for those coordinates. Local or development builds may temporarily
reuse a semantic version while iterating; Package Manager treats a same-version but
different-digest activation as unpublished and can replace it with the exact
repository artifact.

## Publishing a package

1. Update `packages/<slug>/rtwkit.toml` and package sources.
2. Ensure the build produces a valid RTW-layout directory at `artifact-root`.
3. Push a tag matching the package version, for example:

```text
pkg-web-ui-v0.0.1
```

The release workflow builds the package with the canonical Rintawa RTW packer,
publishes the `.rtw` plus machine-readable release metadata, then rebuilds the
Pages registry index.
