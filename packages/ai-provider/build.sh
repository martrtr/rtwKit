#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repository_root"
layout="packages/ai-provider/build"
mkdir -p "$layout"
cargo build --locked --release --target wasm32-unknown-unknown -p rintawa-inference-provider
build_target="${CARGO_TARGET_DIR:-$repository_root/target}"
cargo run --quiet --locked -p rtwkit-componentize -- "$build_target/wasm32-unknown-unknown/release/rintawa_inference_provider.wasm" "$layout/backend.wasm"
npm run build --workspace @rintawa/ai-provider
cargo run --quiet --locked -p rintawa-inference-provider --example portability > "$layout/world-portability.json"
node scripts/assemble-web-package.mjs ai-provider "$layout/backend.wasm" "$layout/world-portability.json"
