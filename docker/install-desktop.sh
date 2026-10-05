#!/bin/bash
# Installe le raccourci « Auto-montage » (menu des applications et bureau) pour ce dépôt.
# Usage : docker/install-desktop.sh
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
chmod +x "$DIR"/docker/*.sh

APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$APPS"
sed "s|@DIR@|$DIR|g" "$DIR/docker/auto-montage.desktop.in" > "$APPS/auto-montage.desktop"
chmod +x "$APPS/auto-montage.desktop"
update-desktop-database "$APPS" >/dev/null 2>&1 || true
echo "Raccourci installé dans le menu : $APPS/auto-montage.desktop"

DESKTOP="$(xdg-user-dir DESKTOP 2>/dev/null || echo "$HOME/Desktop")"
if [ -d "$DESKTOP" ]; then
  cp "$APPS/auto-montage.desktop" "$DESKTOP/"
  chmod +x "$DESKTOP/auto-montage.desktop"
  # GNOME : autorise le lancement sans confirmation.
  gio set "$DESKTOP/auto-montage.desktop" metadata::trusted true >/dev/null 2>&1 || true
  echo "Raccourci copié sur le bureau : $DESKTOP/auto-montage.desktop"
fi
