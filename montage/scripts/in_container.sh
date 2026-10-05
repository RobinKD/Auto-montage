#!/bin/bash
# À sourcer en tête des scripts qui ont besoin des outils du projet (ffmpeg, Whisper, Node).
# Lancé sur la machine, hors du conteneur, sans ces outils : relance le même script dans le
# conteneur Auto-montage (démarré au besoin). Dans le conteneur, ou quand les outils sont
# installés sur la machine (session Claude Code sur le web), ne fait rien.
if [ -z "${AM_IN_CONTAINER:-}" ] && ! python3 -c "import imageio_ffmpeg" >/dev/null 2>&1; then
  _am_script="scripts/$(basename "$0")"
  if [ -f ../docker/lib.sh ]; then
    echo "Outils absents sur cette machine : lancement dans le conteneur Auto-montage…" >&2
    . ../docker/lib.sh
    am_up >&2
    _am_tty=-i
    [ -t 0 ] && [ -t 1 ] && _am_tty=-it
    exec "$AM_ENGINE" exec $_am_tty -u "$AM_UID:$AM_GID" -w /app/montage "$AM_CONTAINER" bash "$_am_script" "$@"
  fi
  echo "Outils absents : pip3 install -r scripts/requirements.txt, ou passer par le conteneur (docker/README.md)." >&2
  exit 1
fi
