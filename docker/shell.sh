#!/bin/bash
# Ouvre un terminal dans le conteneur (dossier montage/), sous l'utilisateur de la machine.
# Ex. : auto-montage prepare MonRush.mov, auto-montage render final
. "$(dirname "$0")/lib.sh"
am_up
exec "$AM_ENGINE" exec -it -u "$AM_UID:$AM_GID" -w /app/montage "$AM_CONTAINER" bash
