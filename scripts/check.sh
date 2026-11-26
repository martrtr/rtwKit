#!/usr/bin/env bash

set -euo pipefail

repository_root="$(git rev-parse --show-toplevel)"
cd "$repository_root"

if git ls-files -- .vscode/ .idea/ | grep -q .; then
    echo "Local editor and planning files must not be tracked." >&2
    exit 1
fi

bash scripts/test-secret-scan.sh
bash scripts/scan-secrets.sh

cargo fmt --all --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo clippy --locked --target wasm32-unknown-unknown -p rintawa-web-runtime -p rintawa-chat -p rintawa-character -p rintawa-narrator -p rintawa-inference-provider -p rintawa-package-manager-extension -p rintawa-world-atlas -p rintawa-memory -- -D warnings
cargo test --locked --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --all-features --no-deps
cargo run --quiet --locked -p rtwkit-registry-tool -- validate-repository .
npm run typecheck
npm run test:bridge
node scripts/story-commit-recovery-test.mjs
npm run build:web
node scripts/assemble-web-package.mjs web-ui
node scripts/assemble-web-package.mjs settings-ui
node scripts/assemble-web-package.mjs dice

bash packages/chat/build.sh

bash packages/character/build.sh

bash packages/narrator/build.sh
bash packages/ai-provider/build.sh

bash packages/package-manager/build.sh

bash packages/world-atlas/build.sh
bash packages/memory/build.sh
