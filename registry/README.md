# Registry output

The public registry is generated from metadata attached to GitHub Releases.
Generated `index.json` is deployed to GitHub Pages and is intentionally not
committed to `main`.

This keeps source history free of generated binary/catalog state while making
published package versions immutable and independently downloadable.

A release contributes one `<slug>.registry.json` file. The registry
tool combines all published metadata into a deterministic package index sorted
by package id and semantic version.
