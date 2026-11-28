#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repository_root"
layout="packages/scenes/build"
mkdir -p "$layout"
cargo build --locked --release --target wasm32-unknown-unknown -p rintawa-scenes
build_target="${CARGO_TARGET_DIR:-$repository_root/target}"
cargo run --quiet --locked -p rtwkit-componentize -- "$build_target/wasm32-unknown-unknown/release/rintawa_scenes.wasm" "$layout/backend.wasm"
npm run build --workspace @rintawa/scenes
cargo run --locked --quiet -p rintawa-scenes --example portability > "$layout/world-portability.json"
node scripts/assemble-web-package.mjs scenes "$layout/backend.wasm" "$layout/world-portability.json"
