#!/bin/bash
# Démarre le conteneur si besoin et ouvre l'interface dans le navigateur.
# Usage : docker/open.sh [downloads/]   (page des moments par défaut)
. "$(dirname "$0")/lib.sh"
URL="$AM_URL/${1:-}"
am_up
# Serveur lancé avant une mise à jour (autre version que les fichiers) : redémarré pour
# servir la nouvelle version.
INSTALLED="$(cat "$AM_ROOT/VERSION" 2>/dev/null || echo dev)"
RUNNING="$(curl -fsS "$AM_URL/api/version" 2>/dev/null | sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' || true)"
if am_ready && [ "$RUNNING" != "$INSTALLED" ]; then
  echo "Mise à jour vers la version $INSTALLED : redémarrage d'Auto-montage…"
  am_compose up -d --force-recreate >/dev/null
  sleep 2
fi
if ! am_ready; then
  echo "Démarrage d'Auto-montage ($(am_engine_name)). La première fois, l'installation prend 10 à 20 min."
  "$AM_ENGINE" logs -f --tail 20 "$AM_CONTAINER" &
  LOGS=$!
  until am_ready; do
    if ! am_running; then
      kill "$LOGS" 2>/dev/null || true
      am_pause "Le conteneur s'est arrêté : voir « $AM_ENGINE logs $AM_CONTAINER »."
      exit 1
    fi
    sleep 3
  done
  kill "$LOGS" 2>/dev/null || true
fi
echo "Interface prête : $URL"
xdg-open "$URL" >/dev/null 2>&1 || open "$URL" 2>/dev/null || echo "Ouvrez $URL dans le navigateur."
