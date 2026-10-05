#!/bin/bash
# Rush d'essai pour les tests automatiques : voix de synthèse en français (espeak-ng), avec
# des blancs, une phrase reprise (1re prise à écarter) et une image de test en 1080x1920.
# Usage : tests/make_sample_rush.sh <fichier de sortie .mov>   (besoin : espeak-ng, ffmpeg)
set -euo pipefail
OUT="$1"
TMP="$(mktemp -d)"
FFMPEG="${FFMPEG:-ffmpeg}"
i=0
for phrase in \
  "Bonjour à tous, aujourd'hui je vous explique comment faire des économies." \
  "Première chose à faire, il faut mettre de côté un peu d'argent." \
  "Première chose à faire, il faut mettre de côté un peu d'argent chaque mois." \
  "Deuxième chose, il faut comparer les prix avant d'acheter." \
  "Et voilà, merci de m'avoir écouté, à bientôt."; do
  espeak-ng -v fr -s 145 -w "$TMP/p$i.wav" "$phrase"
  "$FFMPEG" -v error -y -f lavfi -i anullsrc=r=22050:cl=mono -t 1.2 "$TMP/s$i.wav"
  printf "file 'p%s.wav'\nfile 's%s.wav'\n" "$i" "$i" >> "$TMP/liste.txt"
  i=$((i + 1))
done
"$FFMPEG" -v error -y -f concat -safe 0 -i "$TMP/liste.txt" -ar 48000 -ac 1 "$TMP/voix.wav"
DUREE="$( { "$FFMPEG" -i "$TMP/voix.wav" 2>&1 || true; } | sed -n 's/.*Duration: \([0-9:.]*\).*/\1/p')"
"$FFMPEG" -v error -y -f lavfi -i "testsrc2=size=1080x1920:rate=30" -i "$TMP/voix.wav" -t "$DUREE" \
  -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac -b:a 128k -shortest "$OUT"
rm -rf "$TMP"
echo "Rush d'essai : $OUT ($DUREE)"
