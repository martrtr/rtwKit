# rtwKit packages

Each direct child of this directory is one independently versioned RTW package.
The directory name is its release slug; it is not the Rintawa package identity.

A package may exist in source form before its first release while a runtime adapter is still being implemented. Such a package must remain clearly marked as unreleased and must not emit a placeholder RTW artifact merely to satisfy tooling.

Every package contains `rtwkit.toml`:

```toml
schema = 1

[package]
id = "rintawa.example"
name = "Example"
version = "0.1.0"
description = "Example rtwKit package"
license = "GPL-3.0-only"
authors = ["Example Author"]
logo = "assets/logo.png"
readme = "README.md"

[build]
artifact-root = "build/rtw"
command = ["bash", "build.sh"]
```

`logo` and `readme` are optional bounded presentation assets. The release tooling
publishes them independently from the RTW and records HTTPS URL, SHA-256, size,
and media type in the registry so Extension Manager can show package cards
without downloading or executing the package.

`artifact-root` is the directory that becomes the root of the final `.rtw` ZIP.
It must contain a valid `rtw.toml`. The registry tooling reads the RTW content type
from that file instead of duplicating it in `rtwkit.toml`.

`command` is optional. If present, it is executed directly as an argv array with
the package directory as its working directory. Shell syntax is not interpreted;
use `bash build.sh` explicitly if a package needs a shell.

Release tags use:

```text
pkg-<slug>-v<version>
```

For example `packages/web-ui` version `0.1.0` is released by tag
`pkg-web-ui-v0.1.0`.
