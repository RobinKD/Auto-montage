#!/bin/bash
# Installe whisper.cpp (compilé pour le CPU) et télécharge les modèles utilisés par
# scripts/transcribe_segments.py et prepare.sh. Ne refait rien si tout est déjà là.
# Usage : bash scripts/setup_whisper.sh
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=1.7.6
# WHISPER_DIR : autre emplacement (conteneur Docker : volume /opt/whisper/whisper.cpp).
WHISPER="${WHISPER_DIR:-$PWD/whisper.cpp}"

if [ ! -x "$WHISPER/build/bin/whisper-cli" ] || [ ! -x "$WHISPER/build/bin/vad-speech-segments" ]; then
  if [ ! -d "$WHISPER/.git" ]; then
    rm -rf "$WHISPER"
    git clone --quiet --depth 1 --branch "v$VERSION" https://github.com/ggml-org/whisper.cpp.git "$WHISPER"
  fi
  # Même compilation qu'avant (make : CMake dans build/, binaires dans build/bin/).
  make -C "$WHISPER" -j"$(nproc 2>/dev/null || echo 2)"
fi

# Téléchargés avec curl (passe par le proxy de l'environnement s'il y en a un).
download() {
  [ -f "$WHISPER/$1" ] && return
  curl -sSL --fail --retry 4 -o "$WHISPER/$1.part" "$2"
  mv "$WHISPER/$1.part" "$WHISPER/$1"
}
download ggml-large-v3-turbo.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin
download ggml-silero-v5.1.2.bin https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin
echo "whisper.cpp prêt dans $WHISPER"
