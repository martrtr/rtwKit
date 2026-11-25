# Registry output

GitHub Pages serves a generated `index.json` assembled from current GitHub Release
metadata. Generated catalogs and downloaded metadata are not committed to `main`.

The current writer/reader accepts release/index schema 4 and package manifest
schema 2. Earlier schemas and package-ID replacement aliases were retired with
the old releases; no migration or replacement registry schema is introduced.

One release contributes `<slug>.registry.json`. The tool preserves per-version
artifact digests, dependencies and source provenance, and sorts packages/versions
deterministically. An empty release set produces an empty catalog with repository
identity and its icon. The icon lives here, independently from any package.

Release and Pages workflows remain in `.github/workflows/`. Rebuild Pages after
removing releases: an already deployed index is not changed by local source edits.
