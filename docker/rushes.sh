#!/bin/bash
# Ouvre le dossier où déposer les rushes (montage/public/rushes).
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p montage/public/rushes
xdg-open montage/public/rushes >/dev/null 2>&1 || open montage/public/rushes 2>/dev/null \
  || echo "Dossier des rushes : $(pwd)/montage/public/rushes"
