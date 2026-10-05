#!/bin/bash
# Installe les dépendances du projet (depuis montage/). Idempotent : chaque étape saute ce
# qui existe déjà. Appelé par le hook de session Claude Code sur le web
# (.claude/hooks/session-start.sh) et au démarrage du conteneur Docker (docker/entrypoint.sh).
set -euo pipefail
cd "$(dirname "$0")/.."

# Paquets Node (HyperFrames, GSAP).
if [ ! -d node_modules/hyperframes ] || [ package.json -nt node_modules/.package-lock.json ]; then
  npm install --no-audit --no-fund
fi

# Paquets Python (ffmpeg complet, numpy, OpenCV, Pillow), s'ils manquent.
# (OpenCV 5 n'a plus le détecteur de visage : remplacé par la version 4 de requirements.txt.)
python3 -c "import imageio_ffmpeg, numpy, cv2, PIL; assert hasattr(cv2, 'CascadeClassifier')" 2>/dev/null \
  || pip3 install -q -r scripts/requirements.txt

# Navigateur de rendu de HyperFrames (Chromium de Playwright sur Linux ARM64 : Mac Apple Silicon).
bash scripts/ensure_browser.sh >/dev/null \
  || echo "Navigateur de rendu non installé (réessayé au prochain démarrage et à chaque rendu)."

# Whisper compilé pour le CPU + modèles (environ 1,7 Go en tout, seulement la première fois).
bash scripts/setup_whisper.sh

# Police Oliver (usage personnel, non versionnée).
bash scripts/fetch_fonts.sh

# Modèle de détection du visage (centrage des zooms).
mkdir -p work/models
[ -f work/models/haarcascade_frontalface_default.xml ] || curl -sSL -o work/models/haarcascade_frontalface_default.xml \
  https://raw.githubusercontent.com/opencv/opencv/4.x/data/haarcascades/haarcascade_frontalface_default.xml

echo "Environnement Auto-montage prêt."
