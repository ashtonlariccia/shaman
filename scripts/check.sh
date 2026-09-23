#!/usr/bin/env bash
# Headless verification — no GUI, no focus stealing. Prefer this over launching
# the app when confirming a change compiles and behaves.
set -euo pipefail

cd "$(dirname "$0")/.."

# shellcheck source=scripts/_npm.sh
source "$(dirname "$0")/_npm.sh"

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
npm_run run check

echo "==> frontend tests"
npm_run test

echo "==> all checks passed"
