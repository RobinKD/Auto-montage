#!/usr/bin/env bash
# ffmpeg donné à HyperFrames (render_hyperframes.py, HYPERFRAMES_FFMPEG_PATH) : HyperFrames extrait
# les images de chaque plan avec un ffmpeg par plan, tous lancés en même temps (un montage de
# 150 plans : 150 ffmpeg, plus de 10 Go de mémoire). Ces extractions (sortie « frame_%05d »)
# passent ici une par place libre, AM_FFMPEG_SLOTS à la fois (verrous flock dans AM_FFMPEG_LOCKS) ;
# les autres appels (encodage, son, vérifications) vont directement à ffmpeg (AM_FFMPEG_REAL).
FF="${AM_FFMPEG_REAL:?AM_FFMPEG_REAL manquant}"
case " $* " in
  *frame_%05d*) ;;
  *) exec "$FF" "$@" ;;
esac
if [ -z "${AM_FFMPEG_LOCKS:-}" ] || ! command -v flock >/dev/null 2>&1; then
  exec "$FF" "$@"
fi
mkdir -p "$AM_FFMPEG_LOCKS"
while :; do
  for i in $(seq 1 "${AM_FFMPEG_SLOTS:-2}"); do
    exec {fd}>"$AM_FFMPEG_LOCKS/$i.lock"
    # Verrou gardé par ffmpeg (descripteur hérité) jusqu'à sa fin.
    if flock -n "$fd"; then
      exec "$FF" "$@"
    fi
    exec {fd}>&-
  done
  sleep 0.2
done
