#!/bin/bash
# Construit Auto-montage-<version>-macOS.dmg sans Mac (Linux), quand GitHub Actions n'est pas
# disponible. Même contenu que build-dmg.sh, mais sans les outils d'Apple :
# - l'application lance main.applescript avec osascript (fourni avec macOS) au lieu de le
#   compiler (osacompile) : Contents/MacOS/Auto-montage est un script ;
# - icône faite par Pillow, pas de signature « ad hoc » (inutile pour un script) ;
# - image ISO (xorrisofs, droits Rock Ridge gardés) compressée en .dmg par l'outil « dmg »
#   de libdmg-hfsplus (méthode des versions macOS de Bitcoin Core).
# Prérequis : xorriso, Pillow, et l'outil dmg
#   (git clone https://github.com/fanquake/libdmg-hfsplus && cmake -B build && cmake --build build),
#   passé par DMG_TOOL=<chemin> s'il n'est pas dans le PATH.
# Programme : celui du projet (montage/bin/auto-montage), sinon ceux donnés après le dossier de
# sortie : un seul (universel, ou d'une seule sorte de Mac : auto-montage-macos-arm64 pour Apple
# Silicon), ou plusieurs réunis en un programme universel par llvm-lipo (ou lipo) s'il est
# installé. Pas de signature ici : les programmes compilés sur un Mac (programme.yml) sont déjà
# signés « ad hoc » par l'éditeur de liens, ce qui suffit sur Apple Silicon, et lipo garde
# la signature de chaque partie.
# Usage : packaging/macos/build-dmg-linux.sh <dossier du projet (make-payload.sh)> <dossier de sortie> [programme macOS…]
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
PAYLOAD="$(cd "$1" && pwd)"
OUT="$(mkdir -p "$2" && cd "$2" && pwd)"
VERSION="$(cat "$PAYLOAD/VERSION")"
shift 2
PROGRAMMES=("$@")
LIPO="$(command -v llvm-lipo || command -v lipo || true)"
if [ ${#PROGRAMMES[@]} -eq 0 ] && [ ! -x "$PAYLOAD/montage/bin/auto-montage" ]; then
  echo "Programme absent : $PAYLOAD/montage/bin/auto-montage, ou chemin(s) après le dossier de sortie." >&2; exit 1
fi
if [ ${#PROGRAMMES[@]} -gt 1 ] && [ -z "$LIPO" ]; then
  echo "Plusieurs programmes sans llvm-lipo pour les réunir : n'en donner qu'un (ex. auto-montage-macos-arm64)." >&2; exit 1
fi
DMG_TOOL="${DMG_TOOL:-$(command -v dmg || true)}"
[ -x "$DMG_TOOL" ] || { echo "Outil dmg (libdmg-hfsplus) introuvable : DMG_TOOL=<chemin>." >&2; exit 1; }
ISO_TOOL="$(command -v xorrisofs || true)"
[ -n "$ISO_TOOL" ] || { echo "xorriso manquant (apt install xorriso)." >&2; exit 1; }
WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT
APP="$WORKDIR/dmg/Auto-montage.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# Le script reçoit le chemin de l'application en argument (path to me désignerait le script).
sed -e 's/^on run$/on run argv/' \
    -e 's/^\tset appPath to POSIX path of (path to me)$/\tset appPath to item 1 of argv/' \
    "$HERE/main.applescript" > "$APP/Contents/Resources/main.applescript"
grep -q "item 1 of argv" "$APP/Contents/Resources/main.applescript" \
  || { echo "main.applescript a changé : adapter build-dmg-linux.sh." >&2; exit 1; }
cat > "$APP/Contents/MacOS/Auto-montage" <<'EOF'
#!/bin/bash
APP="$(cd "$(dirname "$0")/../.." && pwd)/"
exec /usr/bin/osascript "$APP/Contents/Resources/main.applescript" "$APP"
EOF
chmod 755 "$APP/Contents/MacOS/Auto-montage"
cp "$HERE/first-run.sh" "$APP/Contents/Resources/first-run.sh"
cp -R "$PAYLOAD" "$APP/Contents/Resources/payload"
BIN="$APP/Contents/Resources/payload/montage/bin/auto-montage"
if [ ${#PROGRAMMES[@]} -gt 0 ]; then
  mkdir -p "$(dirname "$BIN")"
  rm -f "$BIN"
  if [ ${#PROGRAMMES[@]} -eq 1 ]; then cp "${PROGRAMMES[0]}" "$BIN"
  else "$LIPO" -create "${PROGRAMMES[@]}" -output "$BIN"
  fi
fi
chmod 755 "$BIN"
[ -n "$LIPO" ] && echo "Programme : $("$LIPO" -archs "$BIN")"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key><string>fr</string>
	<key>CFBundleExecutable</key><string>Auto-montage</string>
	<key>CFBundleIconFile</key><string>applet</string>
	<key>CFBundleIdentifier</key><string>io.github.robinkd.auto-montage</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>CFBundleName</key><string>Auto-montage</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>$VERSION</string>
	<key>CFBundleVersion</key><string>$VERSION</string>
	<key>LSMinimumSystemVersion</key><string>11.0</string>
	<key>NSAppleEventsUsageDescription</key><string>Auto-montage ouvre le Terminal pour montrer l'avancement.</string>
	<key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
printf 'APPL????' > "$APP/Contents/PkgInfo"

python3 - "$ROOT/icons/icon.png" "$APP/Contents/Resources/applet.icns" <<'EOF'
import sys
from PIL import Image
Image.open(sys.argv[1]).convert("RGBA").resize((1024, 1024), Image.LANCZOS).save(sys.argv[2], format="ICNS")
EOF

ln -s /Applications "$WORKDIR/dmg/Applications"
cp "$HERE/LISEZ-MOI.txt" "$WORKDIR/dmg/LISEZ-MOI - première ouverture.txt"

# Droits gardés tels quels (-R) : fichiers modifiables une fois copiés (mises à jour).
chmod -R u+rwX,go+rX,go-w "$WORKDIR/dmg"
NAME="Auto-montage-$VERSION-macOS.dmg"
"$ISO_TOOL" -quiet -D -l -V "Auto-montage $VERSION" -no-pad -R -o "$WORKDIR/brut.iso" "$WORKDIR/dmg"
"$DMG_TOOL" "$WORKDIR/brut.iso" "$OUT/$NAME" >/dev/null
echo "DMG : $OUT/$NAME"
