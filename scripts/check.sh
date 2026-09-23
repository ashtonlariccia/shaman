#!/usr/bin/env bash
# Headless verification — no GUI, no focus stealing. Prefer this over launching
# the app when confirming a change compiles and behaves.
set -euo pipefail

cd "$(dirname "$0")/.."

# npm has to be reached through Windows either way, but the two dev shells need
# opposite spellings, and each fails silently in the other's.
#
#   WSL:       `cmd.exe /c "npm ..."`. npm.cmd is on the PATH here, but bash
#              tries to run it as a shell script and dies on its line 13.
#   Git Bash:  `npm.cmd` directly. MSYS rewrites cmd.exe's `/c` switch into a
#              path before cmd ever sees it, so cmd opens, runs nothing, and
#              exits 0 -- which had this script reporting "all checks passed"
#              while skipping both frontend steps entirely.
npm_run() {
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) npm.cmd "$@" ;;
    *) cmd.exe /c "npm $*" ;;
  esac
}

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
