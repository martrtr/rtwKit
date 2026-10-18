# Web Runtime

Web Runtime provides the `rintawa.runtime.web-bundle@1` execution target for
packaged web components. It serves validated web assets over loopback HTTP,
connects a WebSocket presentation bridge and works with the Portable UI layer.

The provider is distributed as an RTW package containing a WebAssembly Component
Model component. A web component declares its own capabilities and runtime
permissions in its descriptor.

## Build

```sh
./build.sh
```

The build compiles the provider for `wasm32-unknown-unknown` and produces an
RTW artifact layout. A compatible WebAssembly linker is required.
