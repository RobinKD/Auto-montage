#!/bin/bash
# macOS : copie le projet embarqué dans l'application vers ~/Auto-montage au premier
# lancement ou quand l'application est plus récente que le projet installé (rushes, rendus et
# sélections déjà présents restent).
# Usage : first-run.sh <dossier payload de l'application>
set -euo pipefail
PAYLOAD="$1"
DEST="${AUTO_MONTAGE_HOME:-$HOME/Auto-montage}"
VERSION="$(cat "$PAYLOAD/VERSION")"
# Vrai si la version $1 est plus récente que $2 (0.10 > 0.9) : une version plus récente déjà
# installée (mise à jour depuis l'interface, page « Mises à jour ») n'est jamais remplacée.
version_gt() {
  awk -v a="$1" -v b="$2" 'BEGIN { n = split(a, x, "."); m = split(b, y, "."); if (m > n) n = m
    for (i = 1; i <= n; i++) { if (x[i] + 0 > y[i] + 0) exit 0; if (x[i] + 0 < y[i] + 0) exit 1 } exit 1 }'
}
INSTALLED="$(cat "$DEST/.auto-montage-version" 2>/dev/null || true)"
if [ -z "$INSTALLED" ] || version_gt "$VERSION" "$INSTALLED"; then
  mkdir -p "$DEST"
  cp -R "$PAYLOAD/." "$DEST/"
  chmod -R u+w "$DEST" 2>/dev/null || true
  chmod +x "$DEST"/docker/*.sh "$DEST"/montage/scripts/*.sh
  mkdir -p "$DEST/montage/public/rushes"
  echo "$VERSION" > "$DEST/.auto-montage-version"
fi
echo "$DEST"
