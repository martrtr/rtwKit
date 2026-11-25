#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repository_root"
layout="packages/package-manager/build"
mkdir -p "$layout"
cargo build --locked --release --target wasm32-unknown-unknown -p rintawa-package-manager-extension
build_target="${CARGO_TARGET_DIR:-$repository_root/target}"
cargo run --quiet --locked -p rtwkit-componentize -- "$build_target/wasm32-unknown-unknown/release/rintawa_package_manager_extension.wasm" "$layout/backend.wasm"
npm run build --workspace @rintawa/package-manager
node scripts/assemble-web-package.mjs package-manager "$layout/backend.wasm"
