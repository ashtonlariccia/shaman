#!/usr/bin/env bash
# Launch Shaman in dev mode (Vite HMR + Tauri).
#
# Runs the WINDOWS toolchain from WSL via interop. Never use the Linux `cargo`
# or `node` here — the build target is Windows.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> starting Shaman dev (Windows toolchain via WSL interop)"
echo "    a window will open on the Windows desktop and take focus"

exec cmd.exe /c "npm run tauri -- dev"
