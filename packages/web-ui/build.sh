#!/usr/bin/env bash
set -euo pipefail

package_dir="$(cd "$(dirname "$0")" && pwd)"
cd "$package_dir"

npm ci --include=dev
npm run build:rtw
