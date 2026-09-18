# rtwKit

rtwKit is the recommended package set and package source for Rintawa. It is not
part of Rintawa Core: every package is built and distributed through the same RTW
artifact model available to third-party repositories.

## Distribution model

Source code lives together in this monorepo, but users never need to clone it.
Each package version is published independently as one immutable `.rtw` GitHub
Release asset. A small GitHub Pages `index.json` lists the available versions and
their SHA-256 digests.

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

- `web-runtime` — external `rintawa.runtime.web-bundle@1` execution-target provider.
- `web-ui` — React Host Shell / Portable UI Layer executed through `web-runtime`.

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
