#!/usr/bin/env bash
# Modèles des visages d'opencv_zoo, copies de Hugging Face (scripts/detect_face.py) :
# YuNet (détection, 230 Ko, licence MIT) et SFace (qui est qui, 37 Mo, licence Apache 2.0).
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p work/models
fetch() {  # <fichier> <dépôt Hugging Face>
  local out="work/models/$1"
  [ -s "$out" ] && return 0
  curl -fsSL -o "$out.part" "https://huggingface.co/opencv/$2/resolve/main/$1"
  # Moins de 100 Ko : page d'erreur ou pointeur Git LFS, pas le modèle.
  [ "$(wc -c < "$out.part")" -gt 100000 ] || { rm -f "$out.part"; echo "Modèle $1 : téléchargement invalide" >&2; return 1; }
  mv "$out.part" "$out"
}
fetch face_detection_yunet_2023mar.onnx face_detection_yunet
fetch face_recognition_sface_2021dec.onnx face_recognition_sface || echo "SFace indisponible : personnes non reconnues d'un plan à l'autre." >&2
