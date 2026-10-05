#!/usr/bin/env bash
# Rendu du montage avec HyperFrames (scripts/render_hyperframes.py), en deux temps :
#   ./scripts/render.sh apercu   1080p (format du rush), rapide, pour valider le montage
#   ./scripts/render.sh final    4K une fois le montage validé
# Le mode final encode d'abord la source 4K des seules images gardées (make_hd.py),
# puis rend la même page à l'échelle 2 : texte net et image 4K native.
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/in_container.sh  # hors du conteneur, sans les outils : relancé dans le conteneur

MODE="${1:-}"
case "$MODE" in
  apercu) OUT=out/montage_apercu.mp4 ;;
  final)  OUT=out/montage_4k.mp4 ;;
  *) echo "Usage : $0 apercu|final" >&2; exit 1 ;;
esac

if [ "$MODE" = final ]; then
  python3 scripts/make_hd.py
fi

mkdir -p out
# Rendu sous un nom provisoire, renommé à la fin : un rendu interrompu (« Annuler ») laisse la
# version précédente intacte.
PART="${OUT%.mp4}.part.mp4"
START=$(date +%s)
python3 scripts/render_hyperframes.py "$MODE" "$PART"
mv -f "$PART" "$OUT"
echo "Rendu en $(( $(date +%s) - START )) s"

# Contrôle : dimensions, durée et saturation du son.
python3 - "$OUT" <<'PY'
import subprocess, sys
import imageio_ffmpeg, numpy as np
path = sys.argv[1]
ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()
info = subprocess.run([ffmpeg, "-hide_banner", "-i", path], capture_output=True, text=True).stderr
size = next((w for line in info.splitlines() if "Video:" in line for w in line.split(", ") if "x" in w and w.split(" ")[0].replace("x", "").isdigit()), "?")
raw = subprocess.run([ffmpeg, "-v", "error", "-i", path, "-ac", "1", "-ar", "48000", "-f", "s16le", "-"], capture_output=True).stdout
audio = np.frombuffer(raw, np.int16).astype(float) / 32768
clipped = int(np.sum(np.abs(audio) > 0.98))
print(f"{path} : {size.split(' ')[0]}, {len(audio) / 48000:.1f} s, {clipped} échantillons saturés")
sys.exit(1 if clipped else 0)
PY
