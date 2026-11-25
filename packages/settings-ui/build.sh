#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repository_root"
npm run build --workspace @rintawa/settings-ui
node scripts/assemble-web-package.mjs settings-ui
