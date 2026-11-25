#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repository_root"
layout="packages/web-runtime/build/rtw"
mkdir -p "$layout"
cargo build --locked --release --target wasm32-unknown-unknown -p rintawa-web-runtime
build_target="${CARGO_TARGET_DIR:-$repository_root/target}"
cargo run --quiet --locked -p rtwkit-componentize -- \
    "$build_target/wasm32-unknown-unknown/release/rintawa_web_runtime.wasm" "$layout/runtime.wasm"
cp packages/web-runtime/rtw.toml "$layout/rtw.toml"
cp packages/web-runtime/manifest.toml "$layout/manifest.toml"
