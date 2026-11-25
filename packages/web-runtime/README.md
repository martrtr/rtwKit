# Web Runtime

Web Runtime is the sandboxed execution target for packaged web components. It
serves validated bundle assets over loopback HTTP and exposes the current browser
bridge; it does not implement the retired Portable UI protocol.

The provider is distributed as an RTW package containing a WebAssembly Component
Model component. Each web component declares its capabilities and runtime
permissions in its descriptor.

## Build

```sh
./build.sh
```

The build compiles the provider for `wasm32-unknown-unknown` and produces an
RTW artifact layout. A compatible WebAssembly linker is required.
