#!/usr/bin/env bash
# Télécharge la police Oliver (Iva Florina, dafont) dans public/fonts/.
# Licence : gratuite pour un usage personnel uniquement ; usage commercial
# soumis à licence auprès de l'autrice. Elle n'est donc pas versionnée.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f public/fonts/Oliver-Regular.ttf ] && exit 0
tmp=$(mktemp -d)
curl -sSL -A "Mozilla/5.0" -o "$tmp/oliver.zip" "https://dl.dafont.com/dl/?f=oliver_3"
python3 -c "import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extract('Oliver-Regular.ttf', sys.argv[2])" "$tmp/oliver.zip" public/fonts
rm -rf "$tmp"
echo "Oliver installée dans public/fonts/"
