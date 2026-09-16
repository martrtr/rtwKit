# Rintawa Web UI

Default React UI layer for Rintawa.

```bash
npm ci
npm run dev
```

The bundle talks to Rintawa through a transport-neutral bridge. Platform shells
provide the bridge; development can use WebSocket or the built-in demo host.

Portable UI placement hints are composed by the Web layer into primary/secondary,
sidebar, settings, status, dialog, and overlay regions. They remain renderer hints rather
than host-side layout guarantees.

Build the RTW layout with:

```bash
npm run build:rtw
```
