#!/usr/bin/env bash
# Capture the running Shaman window to a PNG and print the WSL path to it.
#
# Captures ONLY Shaman's window (via PrintWindow), never the full desktop, and
# does not steal focus -- safe to run while other work is in the foreground.
set -euo pipefail

cd "$(dirname "$0")/.."

win_out='C:\Users\Public\shaman-window.png'

powershell.exe -NoProfile -ExecutionPolicy Bypass \
  -File "$(wslpath -w scripts/screenshot.ps1)" \
  -Out "$win_out" | tr -d '\r'

wslpath -u "$win_out"
