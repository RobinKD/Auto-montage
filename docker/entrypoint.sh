#!/bin/bash
# Démarrage du conteneur : installe ce qui manque (première fois : 10 à 20 min, surtout
# Whisper et son modèle), puis lance l'interface web. Toute autre commande est exécutée
# telle quelle (ex. docker compose run --rm app bash).
set -euo pipefail
# Utilisateur : celui donné par les lanceurs, sinon le propriétaire du dépôt monté (Linux,
# docker compose lancé à la main), sinon 1000 (Docker Desktop, où le dépôt paraît à root).
OWNER_UID="$(stat -c %u /app 2>/dev/null || echo 0)"
OWNER_GID="$(stat -c %g /app 2>/dev/null || echo 0)"
if [ "$OWNER_UID" = 0 ] && ! awk 'NR==1 && $1==0 && $2==0 {f=1} END {exit !f}' /proc/self/uid_map; then
  # Podman sans root (espace de noms utilisateur) : root ici = l'utilisateur de la machine.
  OWNER_GID=0
elif [ "$OWNER_UID" = 0 ]; then
  OWNER_UID=1000; OWNER_GID=1000
fi
AM_UID="${AM_UID:-$OWNER_UID}"
AM_GID="${AM_GID:-$OWNER_GID}"
export AM_UID AM_GID

# Démarré en root : prépare les volumes (paquets Node, Whisper, dossier personnel) pour
# l'utilisateur de la machine, puis continue sous son identité. Les fichiers créés dans le
# dépôt lui appartiennent (Linux ; sous macOS et Windows, Docker Desktop s'en charge).
if [ "$(id -u)" = 0 ] && [ "$AM_UID" != 0 ]; then
  for d in /home/app /app/montage/node_modules /opt/whisper; do
    mkdir -p "$d"
    [ "$(stat -c %u "$d")" = "$AM_UID" ] || chown "$AM_UID:$AM_GID" "$d"
  done
  # Clone sous Windows : le bit exécutable des scripts peut manquer.
  chmod +x /app/montage/scripts/*.sh /app/docker/*.sh 2>/dev/null || true
  exec setpriv --reuid="$AM_UID" --regid="$AM_GID" --clear-groups "$0" "$@"
fi

# Dépôt illisible pour cet utilisateur : expliquer plutôt qu'échouer sur « Permission denied ».
if ! cd /app 2>/dev/null || [ ! -r /app/montage/scripts/setup.sh ]; then
  echo "Le conteneur (utilisateur $AM_UID:$AM_GID) ne peut pas lire le dépôt monté dans /app." >&2
  echo "- Propriétaire du dépôt : $(stat -c '%u:%g' /app 2>/dev/null || echo inconnu). Lancez avec" >&2
  echo "  docker/open.sh, ou AM_UID=\$(id -u) AM_GID=\$(id -g) docker compose up -d." >&2
  if [ -e /sys/fs/selinux/enforce ]; then
    echo "- SELinux est actif (Fedora, RHEL…) : compose.yaml monte le dépôt avec « :z » ;" >&2
    echo "  reconstruire et relancer : docker compose up -d --build --force-recreate." >&2
  fi
  exit 1
fi
git config --global --add safe.directory /app 2>/dev/null || true

if [ "${1:-serve}" != "serve" ]; then
  exec "$@"
fi

# Clone sous Windows sans .gitattributes (ancienne copie) : scripts en CRLF, illisibles ici.
if grep -lq $'\r' /app/montage/scripts/*.sh 2>/dev/null; then
  echo "Les scripts de montage/scripts ont des fins de ligne Windows (CRLF)." >&2
  echo "Dans le dépôt : git rm --cached -r . && git reset --hard, puis relancer." >&2
  exit 1
fi

# Journal du démarrage et du serveur, consultable sur la page « Journal » (/logs/) : tout ce
# qui suit y est aussi écrit (l'ancien est gardé à côté au-delà de 5 Mo).
LOG_DIR=/app/montage/work/logs
mkdir -p "$LOG_DIR"
if [ -f "$LOG_DIR/demarrage.log" ] && [ "$(stat -c %s "$LOG_DIR/demarrage.log")" -gt 5000000 ]; then
  mv -f "$LOG_DIR/demarrage.log" "$LOG_DIR/demarrage.prec.log"
fi
exec > >(tee -a "$LOG_DIR/demarrage.log") 2>&1
echo "== Démarrage du conteneur le $(date '+%d/%m/%Y à %H:%M:%S') — Auto-montage $(cat /app/VERSION 2>/dev/null || echo ?)"
export PYTHONUNBUFFERED=1

echo "Préparation de l'environnement (la première fois : 10 à 20 min)…"
bash /app/montage/scripts/setup.sh

# Plugin Claude Code du projet (skills HyperFrames), déclaré dans .claude/settings.json :
# installé une fois dans le dossier personnel (volume).
if [ ! -f "$HOME/.auto-montage-plugins-2" ]; then
  ( claude plugin marketplace add heygen-com/hyperframes && claude plugin install hyperframes@hyperframes \
    && touch "$HOME/.auto-montage-plugins-2" ) >/dev/null 2>&1 || echo "Plugin Claude Code non installé (réessayé au prochain démarrage)"
fi

cd /app/montage
exec python3 scripts/local_server.py --host 0.0.0.0 --port 8080
