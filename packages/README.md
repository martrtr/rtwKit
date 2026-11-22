# RTW packages

There are currently no publishable packages. The experimental runtime, Portable
UI, projection and durable-job adapters have been removed. Reusable native/WASM
libraries live under `crates/` and do not emit placeholder RTWs.

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
