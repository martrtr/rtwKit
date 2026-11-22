#!/usr/bin/env bash

set -euo pipefail

repository_root="$(git rev-parse --show-toplevel)"
cd "$repository_root"

bash scripts/test-secret-scan.sh
bash scripts/scan-secrets.sh

cargo fmt --all --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo clippy --locked --target wasm32-unknown-unknown -p rintawa-web-runtime -p rintawa-chat -- -D warnings
cargo test --locked --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --all-features --no-deps
cargo run --quiet --locked -p rtwkit-registry-tool -- validate-repository .
npm run typecheck
npm run test:bridge
npm run build:web
node scripts/assemble-web-package.mjs web-ui
node scripts/assemble-web-package.mjs dice

bash packages/chat/build.sh
