# Ouvre un terminal dans le conteneur (dossier montage/).
# Ex. : ./scripts/prepare.sh MonRush.mov, ./scripts/render.sh final
. "$PSScriptRoot\common.ps1"
Wait-Docker
docker compose up -d --build
docker compose exec -u "$($env:AM_UID):$($env:AM_GID)" -w /app/montage app bash
