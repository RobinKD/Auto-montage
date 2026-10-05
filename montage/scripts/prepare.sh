#!/usr/bin/env bash
# Prépare un rush : versions de travail, transcription, visage, montage.
# Usage : ./scripts/prepare.sh <fichier du rush dans public/rushes/, ex. MonRush.mov>
#         ./scripts/prepare.sh <vidéo 1> <vidéo 2> […]   plusieurs vidéos pour un même rush,
#         assemblées dans l'ordre (scripts/join_rushes.py) puis préparées comme un seul rush
# Prérequis : npm i, pip install -r scripts/requirements.txt (ou le conteneur Docker/Podman :
# lancé sur la machine sans ces outils, le script se relance dans le conteneur).
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/in_container.sh

if [ $# -lt 1 ]; then
  echo "Usage : $0 <fichier du rush dans public/rushes/> [<vidéo suivante>…]" >&2
  exit 1
fi
if [ $# -gt 1 ]; then
  python3 scripts/join_rushes.py "$@"
  set -- "$(python3 scripts/join_rushes.py --name "$@")"
fi
RUSH="public/rushes/$(basename "$1")"
[ -f "$RUSH" ] || { echo "Rush introuvable : $RUSH" >&2; exit 1; }

# Domaines réseau de l'environnement cloud (session Claude Code sur le web) : signalés seulement.
if [ "${CLAUDE_CODE_REMOTE:-}" = true ]; then bash scripts/check_network.sh || true; fi
FFMPEG=$(python3 -c "import imageio_ffmpeg; print(imageio_ffmpeg.get_ffmpeg_exe())")
mkdir -p work

# Rush en cours : lu par les autres scripts. Quand il change, le montage de l'ancien rush
# (versions de travail, transcription, page des moments, choix, rendus) est mis de côté dans
# work/projets/ (scripts/project.py) : la page « Nouveau rush » le rouvre sans rien recalculer.
# Un autre fichier envoyé sous le même nom (IMG_0001.mov…) compte aussi comme un nouveau rush :
# le nom, la taille et la date du fichier sont comparés à ceux de la dernière préparation.
OLD="$(cat work/rush.txt 2>/dev/null || true)"
KEY="$RUSH $(stat -c '%s %Y' "$RUSH" 2>/dev/null || stat -f '%z %m' "$RUSH")"
OLD_KEY="$(cat work/rush_key.txt 2>/dev/null || true)"
if [ "$OLD" != "$RUSH" ] || { [ -n "$OLD_KEY" ] && [ "$OLD_KEY" != "$KEY" ]; }; then
  python3 scripts/project.py save
  mkdir -p work
fi
echo "$RUSH" > work/rush.txt
echo "$KEY" > work/rush_key.txt

echo "Étape 1/4 : version de travail 1080p (quelques minutes)"
# 1. Version de travail 1080p en MP4 (analyse du visage, extraits, rendu HyperFrames), au format
#    du rush, petit côté à 1080 : 1080x1920 en 9:16, 1920x1080 en 16:9, 1440x1080 en 4:3…
#    (moments_lib.work_size ; repris par tout le montage).
#    (Écrite sous un nom provisoire .part puis renommée : une préparation interrompue ne laisse
#    pas un fichier incomplet que la suivante prendrait pour bon.)
SIZE="$(python3 -c "import sys; sys.path.insert(0, 'scripts'); import moments_lib as m
s = m.display_size(sys.argv[1]); print(*m.work_size(*s) if s else (1080, 1920), sep=':')" "$RUSH")"
# Version de travail d'une autre taille (même rush préparé avant la 0.43, étiré en 1080x1920) : refaite.
if [ -f public/rushes/rush_1080.mp4 ] && [ "$(python3 -c "import sys; sys.path.insert(0, 'scripts'); import moments_lib as m
print(*m.frame_size('.'), sep=':')")" != "$SIZE" ]; then
  rm -f public/rushes/rush_1080.mp4
fi
if [ ! -f public/rushes/rush_1080.mp4 ]; then
  python3 scripts/progress.py ffmpeg "Version de travail (MP4)" "$FFMPEG" -v error -y -i "$RUSH" -map 0:v:0 -map 0:a:0 \
    -vf "scale=$SIZE,setsar=1" -c:v libx264 -preset veryfast -crf 18 -g 15 -pix_fmt yuv420p \
    -c:a aac -b:a 192k public/rushes/rush_1080.part.mp4
  mv public/rushes/rush_1080.part.mp4 public/rushes/rush_1080.mp4
fi

echo "Étape 2/4 : transcription (Whisper, le plus long : environ la durée du rush)"
# 2. Audio 16 kHz mono, segments de parole (VAD Silero) et transcription par segment.
bash scripts/setup_whisper.sh
"$FFMPEG" -v error -y -i "$RUSH" -vn -ac 1 -ar 16000 -c:a pcm_s16le work/rush_16k.wav
WHISPER="${WHISPER_DIR:-whisper.cpp}"  # Docker : /opt/whisper/whisper.cpp
"$WHISPER/build/bin/vad-speech-segments" -f work/rush_16k.wav \
  -vm "$WHISPER/ggml-silero-v5.1.2.bin" -vsd 150 -vp 40 -np > work/vad.txt 2>/dev/null
python3 scripts/transcribe_segments.py

echo "Étape 3/4 : position du visage"
# 3. Position du visage (Haar cascade d'OpenCV).
mkdir -p work/models
[ -f work/models/haarcascade_frontalface_default.xml ] || curl -sSL -o work/models/haarcascade_frontalface_default.xml \
  https://raw.githubusercontent.com/opencv/opencv/4.x/data/haarcascades/haarcascade_frontalface_default.xml
python3 scripts/detect_face.py

echo "Étape 4/4 : bruitages et montage"
# 4. Police Oliver, bruitages provisoires et montage.
bash scripts/fetch_fonts.sh
python3 scripts/make_sfx.py
python3 scripts/build_edit.py
