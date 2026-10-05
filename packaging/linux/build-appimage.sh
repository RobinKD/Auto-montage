#!/bin/bash
# Construit Auto-montage-<version>-x86_64.AppImage (Linux).
# Usage : packaging/linux/build-appimage.sh <dossier du projet (make-payload.sh)> <dossier de sortie>
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
PAYLOAD="$(cd "$1" && pwd)"
OUT="$(mkdir -p "$2" && cd "$2" && pwd)"
VERSION="$(cat "$PAYLOAD/VERSION")"
WORKDIR="$(mktemp -d)"
APPDIR="$WORKDIR/Auto-montage.AppDir"

mkdir -p "$APPDIR"
cp "$HERE/AppRun" "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"
cp "$HERE/auto-montage.desktop" "$APPDIR/"
cp "$ROOT/docker/icon.png" "$APPDIR/auto-montage.png"
cp "$ROOT/docker/icon.png" "$APPDIR/.DirIcon"
cp -a "$PAYLOAD" "$APPDIR/payload"

TOOL="${APPIMAGETOOL:-$WORKDIR/appimagetool}"
if [ ! -x "$TOOL" ]; then
  curl -fsSL -o "$TOOL" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$TOOL"
fi
ARCH=x86_64 "$TOOL" --appimage-extract-and-run --no-appstream "$APPDIR" "$OUT/Auto-montage-$VERSION-x86_64.AppImage"
rm -rf "$WORKDIR"
echo "AppImage : $OUT/Auto-montage-$VERSION-x86_64.AppImage"
