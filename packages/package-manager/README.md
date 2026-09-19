# Package Manager

Repository-backed extension management for Rintawa.

## Features

- Browse extensions from one or more HTTPS repositories.
- Search and filter the aggregated catalog.
- Install and update exact RTW versions with dependency solving.
- Install an extension directly from an HTTPS `.rtw` URL.
- Review requested runtime permissions before composition changes.
- Enable, disable, and uninstall extensions with fail-closed dependency checks.
- Keep repository settings and manager-owned dependency metadata in owner-scoped preferences.

The built-in repository is `https://martrtr.github.io/rtwKit/index.json`.

Package Manager uses only generic Rintawa host capabilities. Repository, SemVer,
update, dependency, and catalog policy remain outside Core.
