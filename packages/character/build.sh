#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repository_root"
layout="packages/character/build"
mkdir -p "$layout"
cargo build --locked --release --target wasm32-unknown-unknown -p rintawa-character
build_target="${CARGO_TARGET_DIR:-$repository_root/target}"
cargo run --quiet --locked -p rtwkit-componentize -- \
    "$build_target/wasm32-unknown-unknown/release/rintawa_character.wasm" "$layout/backend.wasm"
npm run build --workspace @rintawa/character
cargo run --locked --quiet -p rintawa-character --example portability > "$layout/world-portability.json"
node scripts/assemble-web-package.mjs character "$layout/backend.wasm" "$layout/world-portability.json"
