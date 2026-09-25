# World Manager

World Manager is the recommended first-party Rintawa package for browsing persistent worlds, creating new worlds, and requesting their active or inactive lifecycle state through the generic `world-sessions` host capability.

The package owns product policy and Portable UI presentation only. World identity, durable storage, activation authority, permissions, and lifecycle execution remain generic Rintawa Core mechanisms. Another package can replace World Manager without changing Core.

The `0.0.1` package contains the bounded deterministic catalog/controller, Portable UI rendering model, and a thin WASM adapter over the public `world-sessions` and `portable-ui` WIT contracts. The adapter requests only `world-session-read` and `world-session-write`; it does not receive privileged Core integration.
