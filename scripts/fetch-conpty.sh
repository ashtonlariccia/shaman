#!/usr/bin/env bash
# Fetch Microsoft's ConPTY (conpty.dll + OpenConsole.exe) for bundling.
#
# ConPTY is the layer between Shaman and every local shell: it hosts the
# console program and translates what it draws into the VT stream xterm reads.
# The copy inside Windows only moves with OS updates; this one is the same host
# Windows Terminal ships, so resize reflow, passthrough of escape sequences and
# throughput are current on every Windows 10/11 build. portable-pty loads a
# conpty.dll from the app directory in preference to kernel32's, and that
# conpty.dll starts the OpenConsole.exe sitting beside it.
#
# Pinned by version AND hash: the hash is NuGet's own published packageHash, so
# a changed or substituted package fails here rather than shipping. Idempotent
# -- a second run with the files already staged does nothing.
#
# The binaries are not committed (binaries/ is ignored); every build script
# runs this first, the way release.sh stages the helper sidecar.
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="1.24.260710001"
SHA512="38a3117406cf8857ea44089dfc52698ee2fe8a330603f27358a618e09749b19f91b99af0b91305c0e108fa69c41237b189dee17c6ba933303c00d70e42ca180f"
URL="https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/${VERSION}/microsoft.windows.console.conpty.${VERSION}.nupkg"

DEST="crates/shaman-app/binaries/conpty"
STAMP="$DEST/.version"

if [[ -f "$STAMP" && "$(cat "$STAMP")" == "$VERSION" && -f "$DEST/conpty.dll" && -f "$DEST/OpenConsole.exe" ]]; then
  exit 0
fi

echo "==> fetching ConPTY $VERSION"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL -o "$TMP/conpty.nupkg" "$URL"

GOT="$(sha512sum "$TMP/conpty.nupkg" | cut -d' ' -f1)"
if [[ "$GOT" != "$SHA512" ]]; then
  echo "ERROR: ConPTY package hash mismatch" >&2
  echo "  expected $SHA512" >&2
  echo "  got      $GOT" >&2
  exit 1
fi

# A nupkg is a zip. x64 only: Shaman is built for x86_64-pc-windows-msvc.
unzip -q -o "$TMP/conpty.nupkg" \
  "runtimes/win-x64/native/conpty.dll" \
  "build/native/runtimes/x64/OpenConsole.exe" \
  -d "$TMP/x"

mkdir -p "$DEST"
cp "$TMP/x/runtimes/win-x64/native/conpty.dll" "$DEST/conpty.dll"
cp "$TMP/x/build/native/runtimes/x64/OpenConsole.exe" "$DEST/OpenConsole.exe"
echo "$VERSION" >"$STAMP"
echo "    staged in $DEST"
