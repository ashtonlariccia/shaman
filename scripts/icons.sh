#!/usr/bin/env bash
# Regenerate crates/shaman-app/icons/ from the SVG master.
#
# Only needed when the mark changes -- the PNG/ICO/ICNS set is committed, so a
# normal build never runs this. Wants Python with Pillow, and Chrome or Edge
# for the rasterising (set CHROME to point at a different one).
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> rendering icons"
python scripts/icons.py
echo "==> done"
