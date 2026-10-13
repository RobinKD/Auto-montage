#!/bin/bash
# Copie du projet embarquée dans les installateurs : les fichiers suivis par git (fins de
# ligne de .gitattributes appliquées), sans packaging/ ni .github/, plus le fichier VERSION et,
# si on le donne, le programme compilé pour le système de l'installateur
# (montage/bin/auto-montage, ou auto-montage.exe pour un programme Windows).
# Usage : packaging/make-payload.sh <dossier de sortie> [version] [programme]
#   (le programme peut aussi être donné par AM_PROGRAMME=<chemin>). Chemins relatifs : depuis
#   le dossier d'où le script est lancé.
set -euo pipefail
ICI="$PWD"
OUT="$1"
PROGRAMME="${3:-${AM_PROGRAMME:-}}"
case "$OUT" in /*) ;; *) OUT="$ICI/$OUT" ;; esac
case "$PROGRAMME" in ''|/*) ;; *) PROGRAMME="$ICI/$PROGRAMME" ;; esac
cd "$(dirname "$0")/.."
VERSION="${2:-$(cat VERSION)}"
rm -rf "$OUT"
mkdir -p "$OUT"
git archive --format=tar HEAD | tar -x -C "$OUT"
rm -rf "$OUT/packaging" "$OUT/.github" "$OUT/montage/rust/target"
echo "$VERSION" > "$OUT/VERSION"
chmod +x "$OUT"/montage/am
# Anciens lanceurs Docker, s'il en reste dans le dépôt (le programme n'en a pas besoin).
if [ -d "$OUT/docker" ]; then
  find "$OUT/docker" -name '*.sh' -exec chmod +x {} +
fi
if [ -n "$PROGRAMME" ]; then
  [ -f "$PROGRAMME" ] || { echo "Programme introuvable : $PROGRAMME" >&2; exit 1; }
  NOM=auto-montage
  case "$PROGRAMME" in *.exe) NOM=auto-montage.exe ;; esac
  mkdir -p "$OUT/montage/bin"
  cp "$PROGRAMME" "$OUT/montage/bin/$NOM"
  chmod 755 "$OUT/montage/bin/$NOM"
  echo "Programme : montage/bin/$NOM"
fi
echo "Projet $VERSION prêt dans $OUT ($(du -sh "$OUT" | cut -f1))"
