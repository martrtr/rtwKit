# Web UI

Default React UI layer for Rintawa.

```bash
npm ci
npm run dev
```

The bundle talks to Rintawa through a transport-neutral bridge. Platform shells
provide the bridge; development can use WebSocket or the built-in demo host.

Portable UI placement hints are composed by the Web layer into primary/secondary,
sidebar, settings, status, dialog, and overlay regions. They remain renderer
hints rather than host-side layout guarantees.

The Web renderer maps Rintawa semantic design tokens to `--rintawa-ui-*` CSS
variables. Legacy `--rintawa-color-*`, `--rintawa-spacing-*`, and
`--rintawa-radius-*` overrides remain compatibility fallbacks; renderer-only
layout/chrome variables use the separate `--rintawa-web-*` namespace.

Build the RTW layout with:

```bash
npm run build:rtw
```
