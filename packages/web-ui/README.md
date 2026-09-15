# Rintawa Web UI

Default React UI layer for Rintawa.

```bash
npm ci
npm run dev
```

The bundle talks to Rintawa through a transport-neutral bridge. Platform shells
provide the bridge; development can use WebSocket or the built-in demo host.

Build the RTW layout with:

```bash
npm run build:rtw
```
