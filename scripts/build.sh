#!/usr/bin/env bash
# Release build + Windows installer bundle (NSIS / MSI).
set -euo pipefail

cd "$(dirname "$0")/.."

exec cmd.exe /c "npm run tauri -- build"
