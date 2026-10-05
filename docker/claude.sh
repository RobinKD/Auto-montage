#!/bin/bash
# Ouvre Claude Code dans le conteneur, à la racine du dépôt (CLAUDE.md, plugins du projet).
# La première fois, « claude » demande de se connecter (compte claude.ai ou clé d'API).
. "$(dirname "$0")/lib.sh"
am_up
exec "$AM_ENGINE" exec -it -u "$AM_UID:$AM_GID" -w /app "$AM_CONTAINER" claude "$@"
