# rtwKit Package Manager

Repository-backed extension management for Rintawa.

## Features

- Browse extensions from one or more HTTPS repositories.
- Search and filter the aggregated catalog.
- Install and update exact RTW versions with dependency solving.
- Install an extension directly from an HTTPS `.rtw` URL.
- Review requested runtime permissions before composition changes.
- Inspect, approve, and revoke exact local HTTP origins for components requesting `http-request`, separately from granting the runtime capability.
- Enable, disable, and uninstall extensions with fail-closed dependency checks.
- Keep repository settings and manager-owned dependency metadata in owner-scoped preferences.

The built-in repository is `https://martrtr.github.io/rtwKit/index.json`.

Package Manager uses only generic Rintawa host capabilities. Repository, SemVer,
update, dependency, and catalog policy remain outside Core.

## Local HTTP permissions

Select an installed extension in **Manage → Extensions** and find **Local HTTP origins**
under its runtime permissions. The component must request `http-request` in its
manifest and receive that capability before the exact loopback origin can be
approved. Use a full origin such as `http://127.0.0.1:11434`; public HTTP, LAN
addresses, credentials, paths and wildcard origins are not accepted.

Approval and revocation are persisted in the selected baseline or World scope
and apply when the affected runtime is activated again. The editor does not
claim to hot-revoke a running Host session. Changing the selected World does
not transfer approvals to other scopes.
