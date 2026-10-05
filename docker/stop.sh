#!/bin/bash
# Arrête le conteneur (les rushes, rendus et sélections restent dans le dépôt).
. "$(dirname "$0")/lib.sh"
am_require_engine
am_compose stop
