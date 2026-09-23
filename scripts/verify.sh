#!/usr/bin/env bash
# End-to-end smoke check: boots Shaman, confirms the UI reported UI_READY,
# grabs a screenshot, then shuts everything down.
#
# Use this instead of ./scripts/dev.sh when you just need to know the app still
# works -- it is self-terminating, so it won't sit in the foreground.
set -euo pipefail

cd "$(dirname "$0")/.."

powershell.exe -NoProfile -ExecutionPolicy Bypass \
  -File "$(wslpath -w scripts/verify.ps1)" "$@" | tr -d '\r'
