#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repository_root"
layout="packages/memory/build"
mkdir -p "$layout"
cargo run --locked --quiet -p rintawa-memory --example extraction_schemas > "$layout/extraction_schemas.rs"
if ! cmp -s "$layout/extraction_schemas.rs" packages/memory/src/extraction_schemas.rs; then
    cp "$layout/extraction_schemas.rs" packages/memory/src/extraction_schemas.rs
fi
cargo build --locked --release --target wasm32-unknown-unknown -p rintawa-memory
build_target="${CARGO_TARGET_DIR:-$repository_root/target}"
cargo run --quiet --locked -p rtwkit-componentize -- \
    "$build_target/wasm32-unknown-unknown/release/rintawa_memory.wasm" "$layout/backend.wasm"
npm run build --workspace @rintawa/memory
cargo run --locked --quiet -p rintawa-memory --example portability > "$layout/world-portability.json"
node scripts/assemble-web-package.mjs memory "$layout/backend.wasm" "$layout/world-portability.json"
