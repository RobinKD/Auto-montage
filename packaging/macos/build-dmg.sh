#!/bin/bash
# Construit Auto-montage-<version>-macOS.dmg (à lancer sur un Mac : osacompile, sips,
# iconutil, codesign et hdiutil sont fournis avec macOS).
# Le projet doit contenir le programme macOS (montage/bin/auto-montage, make-payload.sh), de
# préférence universel (lipo -create des programmes macos-arm64 et macos-x86_64).
# Usage : packaging/macos/build-dmg.sh <dossier du projet (make-payload.sh)> <dossier de sortie>
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
PAYLOAD="$(cd "$1" && pwd)"
OUT="$(mkdir -p "$2" && cd "$2" && pwd)"
VERSION="$(cat "$PAYLOAD/VERSION")"
[ -x "$PAYLOAD/montage/bin/auto-montage" ] \
  || { echo "Programme absent du projet : $PAYLOAD/montage/bin/auto-montage (make-payload.sh, 3e argument)." >&2; exit 1; }
echo "Programme : $(lipo -archs "$PAYLOAD/montage/bin/auto-montage")"
WORKDIR="$(mktemp -d)"
APP="$WORKDIR/dmg/Auto-montage.app"
mkdir -p "$WORKDIR/dmg"

osacompile -o "$APP" "$HERE/main.applescript"
cp "$HERE/first-run.sh" "$APP/Contents/Resources/first-run.sh"
cp -R "$PAYLOAD" "$APP/Contents/Resources/payload"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier io.github.robinkd.auto-montage" "$APP/Contents/Info.plist" 2>/dev/null \
  || /usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string io.github.robinkd.auto-montage" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $VERSION" "$APP/Contents/Info.plist" 2>/dev/null \
  || /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist"

# Icône (icons/icon.png -> applet.icns).
ICONSET="$WORKDIR/icon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$ROOT/icons/icon.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) "$ROOT/icons/icon.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/applet.icns"

# Signature « ad hoc » (sans certificat) : le programme (obligatoire pour qu'il se lance sur
# Apple Silicon), puis l'application, nécessaire après l'avoir modifiée.
codesign --force --sign - "$APP/Contents/Resources/payload/montage/bin/auto-montage"
codesign --force --deep --sign - "$APP"

ln -s /Applications "$WORKDIR/dmg/Applications"
# Mode d'emploi de la première ouverture (application non notarisée : Gatekeeper la bloque).
cp "$HERE/LISEZ-MOI.txt" "$WORKDIR/dmg/LISEZ-MOI - première ouverture.txt"
hdiutil create -volname "Auto-montage $VERSION" -srcfolder "$WORKDIR/dmg" -ov -format UDZO \
  "$OUT/Auto-montage-$VERSION-macOS.dmg"
rm -rf "$WORKDIR"
echo "DMG : $OUT/Auto-montage-$VERSION-macOS.dmg"
