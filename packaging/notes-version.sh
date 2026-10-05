#!/bin/bash
# Notes de la prochaine version, sans gh ni jeton GitHub : à coller dans « Draft a new release »
# sur github.com (Releases > Draft a new release, tag v<numéro> sur main, « Set as a
# pre-release », coller les notes, Publish).
#
# Usage, depuis le dépôt à jour (git pull) :
#   packaging/notes-version.sh            notes de la version de VERSION
#   packaging/notes-version.sh 0.37       notes d'un autre numéro
#   packaging/notes-version.sh --installateurs [numéro]   partie « Télécharger » de
#       packaging/release-notes.md (version publiée avec ses installateurs, par installateurs.yml)
# Les notes sont affichées et écrites dans dist/notes-v<numéro>.md (non versionné).
#
# Dernière version publiée : le plus grand tag v<numéro> connu de git (GitHub crée le tag en
# publiant ; « git fetch --tags » le récupère). Notes : les sections de CHANGELOG.md plus récentes
# que celle-ci, jusqu'au numéro demandé, comme packaging/publish-release.sh (sans version
# publiée avant : la section du numéro demandé seulement).
set -euo pipefail
cd "$(dirname "$0")/.."

python3 -c "import json" 2>/dev/null || { echo "Python 3 est nécessaire (commande python3)." >&2; exit 1; }
INSTALLERS=false
if [[ "${1:-}" == "--installateurs" ]]; then INSTALLERS=true; shift; fi
V="${1:-$(tr -d '[:space:]' < VERSION)}"
[[ "$V" =~ ^[0-9]+(\.[0-9]+)*$ ]] || { echo "Numéro de version invalide : $V" >&2; exit 1; }
grep -q "^## $V\$" CHANGELOG.md || { echo "CHANGELOG.md n'a pas de section « ## $V »." >&2; exit 1; }

# Tags publiés : ceux de GitHub si le dépôt distant répond (sans jeton pour un dépôt public ;
# pour un dépôt privé, git utilise vos identifiants habituels), sinon ceux déjà connus.
git fetch --quiet --tags origin 2>/dev/null || echo "(dépôt distant injoignable : tags déjà connus utilisés)" >&2
TAGS="$(git tag --list 'v*')"

mkdir -p dist
OUT="dist/notes-v$V.md"
TAGS="$TAGS" INSTALLERS="$INSTALLERS" python3 - "$V" "$OUT" <<'PY'
import os
import re
import sys

v, out = sys.argv[1:3]
key = lambda s: tuple(int(x) for x in re.findall(r"\d+", s))
tags = [t.strip()[1:] for t in os.environ["TAGS"].split() if re.fullmatch(r"v\d+(\.\d+)*", t.strip())]
older = [t for t in tags if key(t) < key(v)]
# Aucune version publiée avant (dépôt neuf) : seulement la section de ce numéro.
last = max(older, key=key, default=None)
if v in tags:
    print(f"(le tag v{v} existe déjà : cette version semble déjà publiée)", file=sys.stderr)
sections, cur = [], None
for line in open("CHANGELOG.md", encoding="utf-8"):
    m = re.match(r"^## (\d+(?:\.\d+)*)\s*$", line)
    if m:
        cur = m[1] if (key(m[1]) == key(v) if last is None else key(last) < key(m[1]) <= key(v)) else None
        if cur:
            sections.append([cur, []])
        continue
    if cur:
        sections[-1][1].append(line)
parts = ["".join(lines).strip() if num == v else f"### Version {num}\n\n" + "".join(lines).strip()
         for num, lines in sections]
download = ("## Télécharger\n\n"
        "Pas d'installateurs pour cette version. Pour une première installation : ceux de la dernière "
        "version qui en a ([toutes les versions](https://github.com/RobinKD/Auto-montage/releases)), puis "
        f"page **Mises à jour** de l'interface, qui installe la version {v}.\n")
if os.environ["INSTALLERS"] == "true":
    download = open("packaging/release-notes.md", encoding="utf-8").read()
body = "## Nouveautés\n\n" + "\n\n".join(parts) + "\n\n" + download
open(out, "w", encoding="utf-8").write(body)
print(body)
print(f"Depuis la version {last or '(aucune)'} ; notes écrites dans {out}.", file=sys.stderr)
PY

$INSTALLERS && exit 0
cat >&2 <<EOF

Pour publier sur github.com : Releases > Draft a new release
  - Choose a tag : v$V (« Create new tag »), Target : main (poussée avec cette version)
  - Release title : Auto-montage $V
  - Coller les notes ci-dessus (ou le contenu de $OUT)
  - Cocher « Set as a pre-release », puis « Publish release »
EOF
