#!/usr/bin/env bash
# Release build + Windows installer bundle (NSIS / MSI).
set -euo pipefail

cd "$(dirname "$0")/.."

# shellcheck source=scripts/_npm.sh
source "$(dirname "$0")/_npm.sh"

# conpty.dll + OpenConsole.exe are bundle resources; tauri-build fails without them.
bash "$(dirname "$0")/fetch-conpty.sh"

npm_run run tauri -- build
