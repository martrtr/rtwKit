#!/usr/bin/env bash

set -euo pipefail

repository_root="$(git rev-parse --show-toplevel)"
cd "$repository_root"

bash scripts/test-secret-scan.sh
bash scripts/scan-secrets.sh

cargo fmt --all --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo doc --locked --workspace --all-features --no-deps
cargo run --quiet --locked -p rtwkit-registry-tool -- validate-repository .

if [[ -d packages/web-ui/node_modules ]]; then
    (
        cd packages/web-ui
        npm run typecheck
        npm test
    )
else
    echo "packages/web-ui/node_modules is absent; skipping local Web UI checks." >&2
fi
