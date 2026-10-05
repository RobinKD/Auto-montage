#!/bin/bash
# macOS : crée l'application « Auto-montage » (dans ~/Applications, avec un alias sur le
# bureau). Elle propose : moments et version de travail, téléchargements, Claude Code,
# terminal du conteneur, arrêt. Chaque choix s'ouvre dans le Terminal.
# Usage : ./docker/install-macos.sh      (Docker Desktop doit être installé)
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
chmod +x "$DIR"/docker/*.sh
APP="$HOME/Applications/Auto-montage.app"
mkdir -p "$HOME/Applications"

SCRIPT="$(mktemp -t auto-montage).applescript"
cat > "$SCRIPT" <<APPLESCRIPT
set repo to "$DIR"
set choix to choose from list {"Moments et version de travail", "Téléchargements (720p, 1080p, 4K)", "Claude Code", "Terminal du conteneur", "Arrêter Auto-montage"} with title "Auto-montage" with prompt "Que voulez-vous ouvrir ?" default items {"Moments et version de travail"} OK button name "Ouvrir" cancel button name "Annuler"
if choix is false then return
set c to item 1 of choix
if c starts with "Moments" then
	set cmd to quoted form of (repo & "/docker/open.sh")
else if c starts with "Téléchargements" then
	set cmd to quoted form of (repo & "/docker/open.sh") & " downloads/"
else if c is "Claude Code" then
	set cmd to quoted form of (repo & "/docker/claude.sh")
else if c starts with "Terminal" then
	set cmd to quoted form of (repo & "/docker/shell.sh")
else
	set cmd to quoted form of (repo & "/docker/stop.sh")
end if
tell application "Terminal"
	activate
	do script cmd
end tell
APPLESCRIPT

rm -rf "$APP"
osacompile -o "$APP" "$SCRIPT"
rm -f "$SCRIPT"

# Icône : docker/icon.png -> .icns (outils fournis avec macOS).
ICONSET="$(mktemp -d)/icon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$DIR/docker/icon.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) "$DIR/docker/icon.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
if iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/applet.icns" 2>/dev/null; then
  touch "$APP"
fi

ln -sfn "$APP" "$HOME/Desktop/Auto-montage"
echo "Application installée : $APP (alias sur le bureau)."
echo "Au premier lancement, macOS peut demander d'autoriser le contrôle du Terminal : acceptez."
