#!/bin/bash
# Commun aux lanceurs Linux et macOS : dossier du dépôt, moteur de conteneurs (Docker ou
# Podman), utilisateur du conteneur. À sourcer : . "$(dirname "$0")/lib.sh"
# AM_ENGINE=docker|podman force le moteur ; AM_PORT change le port (8080).
set -euo pipefail
AM_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$AM_ROOT"
AM_PORT="${AM_PORT:-8080}"
AM_URL="http://localhost:$AM_PORT"
AM_CONTAINER=auto-montage
export AM_PORT

# Podman : soit la commande podman, soit le paquet « podman-docker » (docker = podman).
# Préférence : celui qui répond (Docker d'abord), sinon celui qui est installé.
if [ -z "${AM_ENGINE:-}" ]; then
  HAS_DOCKER=""; HAS_PODMAN=""
  if command -v docker >/dev/null 2>&1 && ! docker --version 2>/dev/null | grep -qi podman; then HAS_DOCKER=1; fi
  if command -v podman >/dev/null 2>&1; then HAS_PODMAN=1; fi
  if [ -n "$HAS_DOCKER" ] && docker info >/dev/null 2>&1; then AM_ENGINE=docker
  elif [ -n "$HAS_PODMAN" ] && podman info >/dev/null 2>&1; then AM_ENGINE=podman
  elif [ -n "$HAS_DOCKER" ]; then AM_ENGINE=docker  # Docker Desktop arrêté : démarré plus bas
  elif [ -n "$HAS_PODMAN" ]; then AM_ENGINE=podman
  else AM_ENGINE=""
  fi
fi

# Utilisateur du conteneur. Docker : celui de la machine (fichiers du dépôt à son nom).
# Podman sans root : root dans le conteneur correspond déjà à l'utilisateur de la machine.
if [ "$AM_ENGINE" = podman ] && [ "$(id -u)" != 0 ]; then
  export AM_UID=0 AM_GID=0
else
  export AM_UID="$(id -u)" AM_GID="$(id -g)"
fi

am_engine_name() { [ "$AM_ENGINE" = podman ] && echo Podman || echo Docker; }

# Le moteur répond-il ? (Docker Desktop peut être arrêté ; Podman n'a pas de service.)
am_engine_ready() { [ -n "$AM_ENGINE" ] && "$AM_ENGINE" info >/dev/null 2>&1; }

am_compose() {
  if [ "$AM_ENGINE" = docker ]; then
    docker compose "$@"
  elif command -v podman-compose >/dev/null 2>&1; then
    podman-compose "$@"
  else
    podman compose "$@"
  fi
}

am_running() { [ -n "$("$AM_ENGINE" ps -q --filter "name=^${AM_CONTAINER}\$" --filter status=running 2>/dev/null)" ]; }
am_ready() { curl -fsS "$AM_URL/api/regen" >/dev/null 2>&1; }

am_pause() { echo "$1"; echo "Appuyez sur Entrée pour fermer."; read -r _ || true; }

# Moteur prêt (démarre Docker Desktop sous macOS), sinon explique quoi installer.
am_require_engine() {
  if [ -z "$AM_ENGINE" ]; then
    if [ "$(uname)" = Darwin ]; then
      am_pause "Docker Desktop n'est pas installé : https://www.docker.com/products/docker-desktop/"
    else
      am_pause "Ni Docker ni Podman ne sont installés. Exemples : « sudo apt install podman podman-compose » (Debian, Ubuntu), « sudo dnf install podman podman-compose » (Fedora), ou Docker : https://docs.docker.com/engine/install/"
    fi
    exit 1
  fi
  am_engine_ready && return 0
  if [ "$(uname)" = Darwin ] && [ "$AM_ENGINE" = docker ] && open -a Docker 2>/dev/null; then
    echo "Démarrage de Docker Desktop…"
    until am_engine_ready; do sleep 3; done
    return 0
  fi
  if [ "$AM_ENGINE" = docker ]; then
    am_pause "Docker ne répond pas : démarrez-le (« sudo systemctl start docker », ou Docker Desktop), ou vérifiez que votre utilisateur est dans le groupe docker. Podman marche aussi : AM_ENGINE=podman."
  else
    am_pause "Podman ne répond pas : « podman info » donne le détail."
  fi
  exit 1
}

# Démarre le conteneur (image reconstruite si le dépôt a changé : quelques secondes).
am_up() {
  am_require_engine
  # AM_BUILD=0 : ne pas reconstruire l'image (machine hors ligne, image déjà construite).
  local build=--build
  [ "${AM_BUILD:-1}" = 0 ] && build=""
  if ! am_compose up -d $build; then
    am_pause "Le démarrage a échoué (voir les messages ci-dessus)."
    exit 1
  fi
  # Ancienne version de l'image, sans nom depuis la reconstruction : plusieurs Go à chaque mise à jour.
  "$AM_ENGINE" image prune -f --filter label=auto-montage >/dev/null 2>&1 || true
}
