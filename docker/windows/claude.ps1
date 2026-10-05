# Ouvre Claude Code dans le conteneur, à la racine du dépôt (CLAUDE.md, plugins du projet).
# La première fois, « claude » demande de se connecter (compte claude.ai ou clé d'API).
. "$PSScriptRoot\common.ps1"
Wait-Docker
docker compose up -d --build
docker compose exec -u "$($env:AM_UID):$($env:AM_GID)" -w /app app claude @args
