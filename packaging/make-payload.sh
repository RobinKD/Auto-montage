#!/bin/bash
# Copie du projet embarquée dans les installateurs : les fichiers suivis par git (fins de
# ligne de .gitattributes appliquées), sans packaging/ ni .github/, plus le fichier VERSION.
# Usage : packaging/make-payload.sh <dossier de sortie> [version]
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="$1"
VERSION="${2:-$(cat VERSION)}"
rm -rf "$OUT"
mkdir -p "$OUT"
git archive --format=tar HEAD | tar -x -C "$OUT"
rm -rf "$OUT/packaging" "$OUT/.github"
echo "$VERSION" > "$OUT/VERSION"
chmod +x "$OUT"/docker/*.sh "$OUT"/montage/scripts/*.sh
echo "Projet $VERSION prêt dans $OUT ($(du -sh "$OUT" | cut -f1))"
