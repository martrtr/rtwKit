# Package distribution

## RTW container

`.rtw` is a ZIP container with one root content type. Its root `rtw.toml` should
remain intentionally tiny, for example:

```toml
format = 1
content = "rintawa.extension@1"
entry = "manifest.toml"
```

The RTW manifest identifies the container/content protocol only. Extension
versions, package metadata, dependencies, UI assets, and other handler-specific
information belong to their own descriptors.

Production distribution always uses immutable RTW artifacts. Core addresses them
by SHA-256 and does not overwrite an older version when a newer one is imported.

## Authored vs generated formats

Use TOML for human-authored project configuration:

- `rtw.toml` — RTW root descriptor;
- extension `manifest.toml` — runtime descriptor;
- `rtwkit.toml` — rtwKit build/publishing metadata.

Use JSON for generated network/distribution records:

- `<slug>.registry.json` — immutable metadata attached to one release;
- `index.json` — generated package-source catalog.

JSON here is a wire format, not a replacement for TOML project configuration.## Releases

Each package is versioned independently with a tag:

```text
pkg-<slug>-v<version>
```

A release uses stable asset names:

```text
<slug>.rtw
<slug>.registry.json
```

The version must not be duplicated in the asset filename. Exact version identity
comes from the release tag and the package metadata; exact byte identity comes
from SHA-256.

Do not use GitHub repository-wide `releases/latest` for per-package update logic.
In a monorepo the latest GitHub release may belong to another package.

GitHub Release assets are the binary distribution backend. GitHub Actions
artifacts are CI outputs only and are not a stable package source.

## Registry

GitHub Pages publishes a generated `index.json`. The Extension Manager should be
able to check the entire rtwKit source with one HTTP request, compare installed
versions locally, and download only artifacts that are needed.

`latest` means the highest stable SemVer release. A prerelease may be latest only
when the package has no stable release. Full version history remains available.
A release metadata file exists so registry rebuilding never needs to download and
rehash every historical RTW asset. It records package identity/version, content
type, artifact URL, SHA-256, size, source repository, tag, and source commit.

The registry schema belongs to the rtwKit package source. Core must not require it.
Other repositories or managers may use different source protocols.

If the catalog eventually becomes large, `index.json` may be split into a small
root index plus per-package version documents without changing RTW or Core.

## Manual installation

Extension Manager is optional. A user must remain able to download `<slug>.rtw`
from a GitHub Release and import it through a primitive Rintawa Core/CLI operation.
Core validates the RTW and stores it by digest; it does not discover updates.

Manual update means downloading/importing a newer RTW and changing activation.
The old artifact remains available until future garbage-collection policy decides
that no retained generation references it.

## Integrity

Registry SHA-256 is an expected download digest. The downloaded RTW must also be
validated and hashed by Core before entering the Artifact Store. A mismatch is a
hard failure, not a warning.
