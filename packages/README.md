# RTW packages

The experimental runtime, Portable UI, projection and durable-job adapters have
been removed. Their P10 replacements are real packages, with local browser/protocol acceptance:

- `web-runtime`: ordinary WASM provider for `rintawa.runtime.web-bundle@2`.
- `web-ui`: React/Dockview shell, explicit World lifecycle and saved layouts.
- `dice`: independent Canvas/queryable panel demonstrating the browser SDK.
- `chat`: persistent conversations/messages with an ordinary WASM/System owner.

The web packages use the runtime provider; each has its own immutable RTW.
Reusable native/WASM libraries live under `crates/`. Browser authoring code lives
under `web/`; it is build-time code, rather than an extra installed service.

A new independently versioned extension belongs in `packages/<slug>/`. Its real
build must produce a valid RTW root before a release can be created. For example:

```toml
schema = 2

[package]
id = "rintawa.rtwkit.example"
name = "Example"
version = "0.1.0"
description = "Example rtwKit package"
license = "GPL-3.0-only"
authors = ["Example Author"]
readme = "README.md"

[build]
artifact-root = "build/rtw"
command = ["bash", "build.sh"]
```

Every direct child directory must be a complete package with `rtwkit.toml`.
Schema 1 and old package-ID replacement aliases are no longer supported.
Optional bounded `logo` and `readme` assets are published with HTTPS URL,
SHA-256, size and media type so a manager can display them without executing RTW.

The build command is an argv array executed in the package directory. Shell
syntax is not interpreted; invoke `bash` explicitly when needed. Production
installation must never execute source build scripts.

Tag `pkg-<slug>-v<version>` publishes stable asset names `<slug>.rtw` and
`<slug>.registry.json`. The tag, version and digest identify immutable bytes;
do not reuse the removed experimental releases' version coordinates.
