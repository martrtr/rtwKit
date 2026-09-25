# World Manager

World Manager is the recommended first-party Rintawa package for browsing persistent worlds, creating new worlds, and requesting their active or inactive lifecycle state through the generic `world-sessions` host capability.

The package owns product policy and Portable UI presentation only. World identity, durable storage, activation authority, permissions, and lifecycle execution remain generic Rintawa Core mechanisms. Another package can replace World Manager without changing Core.

The current `0.0.1` source slice contains the bounded deterministic catalog/controller and Portable UI rendering model. The thin WASM adapter that binds those domain APIs to the public `world-sessions` and `portable-ui` WIT imports is still pending, so release builds do not emit a placeholder RTW artifact until that adapter exists.
