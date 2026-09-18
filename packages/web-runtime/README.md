# Rintawa Web Runtime

External execution-target provider for packaged Web components.

The package is an ordinary `rintawa.extension@1` artifact whose root WASM
component publishes:

```text
rintawa.runtime.web-bundle@1
```

It does not receive privileged Core integration. Hosted Web components use the
same manifest-requested, host-granted runtime permissions as any other
component. The current Web UI requests `background-task` and
`loopback-listen`; those grants belong to the Web UI component itself and are
not inherited from this provider.

The runtime serves validated bundle assets on loopback HTTP, implements the
WebSocket renderer bridge, and uses the generic Portable UI Layer WIT boundary
for presentation snapshots and semantic actions. Product roles are never
inferred from the Web execution target: each Web descriptor explicitly lists
its provided contracts with `[[provides]]`, while `[ui-layer]` only describes
renderer capabilities when that component actually implements a UI Layer.

Build the RTW layout with:

```bash
./build.sh
```

The build compiles the provider for `wasm32-unknown-unknown` and wraps the core
module as a WebAssembly Component Model component. An explicit Cargo linker may
be supplied by the environment; the script also discovers common linker names
and a Nix-store `wasm-ld` as a local fallback.
