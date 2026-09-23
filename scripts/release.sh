#!/usr/bin/env bash
# Build the standalone Shaman executable.
#
# This is the binary to double-click. Unlike a debug build, it embeds the UI, so
# it needs no dev server -- and it runs without a console window.
#
# Pass --bundle to also produce the NSIS/MSI installers.
set -euo pipefail

cd "$(dirname "$0")/.."

# shellcheck source=scripts/_npm.sh
source "$(dirname "$0")/_npm.sh"

# The helper must sit beside shaman.exe: an elevated Shaman cannot open an
# ordinary terminal without it. `tauri build` only builds the app crate.
echo "==> building shaman-helper"
cargo.exe build --release -p shaman-helper

# Stage it as a Tauri sidecar so the INSTALLER carries it too. Tauri requires the
# target triple in the filename and strips it when bundling, landing the binary
# next to shaman.exe -- exactly where helper_path() looks.
TRIPLE="$(rustc.exe -vV | sed -n 's/^host: //p' | tr -d '\r')"
echo "==> staging sidecar for $TRIPLE"
mkdir -p crates/shaman-app/binaries
cp target/release/shaman-helper.exe "crates/shaman-app/binaries/shaman-helper-${TRIPLE}.exe"

if [[ "${1:-}" == "--bundle" ]]; then
  npm_run run tauri -- build
else
  npm_run run tauri -- build --no-bundle
fi

if [[ ! -f target/release/shaman-helper.exe ]]; then
  echo "ERROR: shaman-helper.exe missing; normal terminals would fail when elevated" >&2
  exit 1
fi

echo
echo "==> standalone executable:"
echo "    $(pwd)/target/release/shaman.exe"
echo "    C:\\Users\\LaRiccia\\Desktop\\shaman\\target\\release\\shaman.exe"
