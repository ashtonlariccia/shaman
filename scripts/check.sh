#!/usr/bin/env bash
# Headless verification — no GUI, no focus stealing. Prefer this over launching
# the app when confirming a change compiles and behaves.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> cargo fmt --check"
cargo.exe fmt --all -- --check || {
  echo "    formatting drift -- run: cargo.exe fmt --all" >&2
  exit 1
}

echo "==> cargo clippy"
cargo.exe clippy --workspace --all-targets -- -D warnings

echo "==> cargo test"
cargo.exe test --workspace

echo "==> svelte-check"
cmd.exe /c "npm run check"

echo "==> frontend tests"
cmd.exe /c "npm test"

echo "==> all checks passed"
