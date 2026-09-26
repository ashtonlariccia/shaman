#!/usr/bin/env bash
# Headless verification — no GUI, no focus stealing. Prefer this over launching
# the app when confirming a change compiles and behaves.
set -euo pipefail

cd "$(dirname "$0")/.."

# shellcheck source=scripts/_npm.sh
source "$(dirname "$0")/_npm.sh"

# conpty.dll + OpenConsole.exe are bundle resources; tauri-build fails without them.
bash "$(dirname "$0")/fetch-conpty.sh"

echo "==> cargo fmt --check"
cargo.exe fmt --all -- --check || {
  echo "    formatting drift -- run: cargo.exe fmt --all" >&2
  exit 1
}

echo "==> cargo clippy"
cargo.exe clippy --workspace --all-targets -- -D warnings

echo "==> cargo test"
# Test binaries run from target/debug/deps, so that is their "app directory":
# staging the bundled host there makes the PTY tests exercise it, not the OS one.
mkdir -p target/debug/deps
cp crates/shaman-app/binaries/conpty/conpty.dll crates/shaman-app/binaries/conpty/OpenConsole.exe target/debug/deps/
SHAMAN_EXPECT_BUNDLED_CONPTY=1 cargo.exe test --workspace

echo "==> svelte-check"
npm_run run check

echo "==> frontend tests"
npm_run test

echo "==> all checks passed"
