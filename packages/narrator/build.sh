#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repository_root"
layout="packages/narrator/build/rtw"
mkdir -p "$layout"
cargo build --locked --release --target wasm32-unknown-unknown -p rintawa-narrator
build_target="${CARGO_TARGET_DIR:-$repository_root/target}"
cargo run --quiet --locked -p rtwkit-componentize -- "$build_target/wasm32-unknown-unknown/release/rintawa_narrator.wasm" "$layout/backend.wasm"
cp packages/narrator/rtw.toml "$layout/rtw.toml"
cp packages/narrator/manifest.toml "$layout/manifest.toml"
cargo run --quiet --locked -p rintawa-narrator --example portability > "$layout/world-portability.json"
