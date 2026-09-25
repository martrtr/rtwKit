#!/usr/bin/env bash
set -euo pipefail

npm ci --include=dev
npm run build:rtw
