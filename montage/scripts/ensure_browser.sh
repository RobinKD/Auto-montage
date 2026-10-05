#!/bin/bash
# Navigateur de rendu de HyperFrames (Chrome sans écran) : affiche son chemin en dernière
# ligne, après l'avoir téléchargé s'il manque. Appelé par setup.sh et render_lib.headless_chrome.
# - Environnement cloud : le Chromium fourni (/opt/pw-browsers), sans téléchargement.
# - Linux ARM64 (conteneur Docker sur un Mac Apple Silicon) : Google ne publie pas de
#   « chrome-headless-shell » pour cette plateforme, et HyperFrames essaie alors un apt-get
#   impossible ici. On prend celui de Playwright (même Chrome 141 que l'environnement cloud).
# - Ailleurs : « hyperframes browser ensure » (Chrome téléchargé dans ~/.cache/hyperframes).
set -euo pipefail
cd "$(dirname "$0")/.."
PLAYWRIGHT_VERSION=1.56.1
PW_DIR="${HOME:-/tmp}/.cache/ms-playwright"

find_shell() {
  for p in /opt/pw-browsers/chromium_headless_shell-*/chrome-linux/headless_shell \
           "$PW_DIR"/chromium_headless_shell-*/chrome-linux/headless_shell; do
    [ -x "$p" ] && { echo "$p"; return 0; }
  done
  return 1
}

find_shell && exit 0

if [ "$(uname -s)" = Linux ] && [ "$(uname -m)" = aarch64 -o "$(uname -m)" = arm64 ]; then
  echo "Navigateur de rendu pour Linux ARM64 : téléchargement du Chromium de Playwright…" >&2
  PLAYWRIGHT_BROWSERS_PATH="$PW_DIR" PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD= \
    npx -y "playwright@$PLAYWRIGHT_VERSION" install --only-shell chromium >&2
  find_shell && exit 0
  echo "Chromium de Playwright introuvable après le téléchargement ($PW_DIR)." >&2
  exit 1
fi

HYPERFRAMES_NO_TELEMETRY=1 node node_modules/hyperframes/dist/cli.js browser ensure
