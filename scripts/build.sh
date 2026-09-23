#!/usr/bin/env bash
# Release build + Windows installer bundle (NSIS / MSI).
set -euo pipefail

cd "$(dirname "$0")/.."

# shellcheck source=scripts/_npm.sh
source "$(dirname "$0")/_npm.sh"

npm_run run tauri -- build
