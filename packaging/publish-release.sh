#!/bin/bash
# Publie la version de VERSION sur GitHub (tag v<numéro> + notes), sans GitHub Actions ni
# installateurs : de quoi faire la mise à jour depuis l'interface (page « Mises à jour », qui
# n'a besoin que du tag). Les installateurs restent ceux de la dernière version qui en a.
#
# Usage, depuis le dépôt, après avoir poussé le commit à publier :
#   packaging/publish-release.sh            (publie)
#   packaging/publish-release.sh --essai    (affiche les notes, ne publie rien)
# Accès à GitHub : commande « gh » connectée (gh auth login) si elle est là, sinon un jeton
# dans GH_TOKEN ou GITHUB_TOKEN, sinon le jeton est demandé (droit « Contents : Read and
# write » sur le dépôt ; le jeton en lecture seule des mises à jour ne suffit pas).
#
# Notes : les sections de CHANGELOG.md plus récentes que la dernière version publiée (une
# version jamais publiée n'est pas perdue), puis le lien vers les installateurs.
set -euo pipefail
cd "$(dirname "$0")/.."
REPO="${AM_REPO:-RobinKD/Auto-montage}"
DRY=false
if [[ "${1:-}" == "--essai" ]]; then DRY=true; fi

python3 -c "import json" 2>/dev/null || { echo "Python 3 est nécessaire (commande python3)." >&2; exit 1; }
V="$(tr -d '[:space:]' < VERSION)"
[[ "$V" =~ ^[0-9]+(\.[0-9]+)*$ ]] || { echo "Numéro de version invalide dans VERSION : $V" >&2; exit 1; }
grep -q "^## $V\$" CHANGELOG.md || { echo "CHANGELOG.md n'a pas de section « ## $V »." >&2; exit 1; }
echo "Version à publier : $V (branche $(git rev-parse --abbrev-ref HEAD), commit $(git rev-parse --short HEAD))"

# Accès à GitHub, choisi une fois : gh connecté, sinon jeton (variable ou saisi).
TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
USE_GH=false
if [[ -z "$TOKEN" ]] && command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  USE_GH=true
  echo "Accès à GitHub : commande gh"
elif [[ -z "$TOKEN" ]]; then
  if [[ ! -t 0 ]]; then
    echo "Pas d'accès à GitHub : connectez « gh » (gh auth login) ou mettez un jeton dans GH_TOKEN." >&2
    exit 1
  fi
  echo "Pas de commande « gh » connectée. Collez un jeton GitHub avec le droit « Contents : Read and write »"
  echo "sur $REPO (github.com > Settings > Developer settings > Fine-grained tokens), puis Entrée :"
  read -rs TOKEN
  echo
  [[ -n "$TOKEN" ]] || { echo "Aucun jeton : rien n'est publié." >&2; exit 1; }
fi

api() {  # api <méthode> <chemin> [fichier JSON] ; échec : code non nul, message sur stderr
  if $USE_GH; then
    gh api -X "$1" "repos/$REPO$2" ${3:+--input "$3"}
  else
    curl -fsS -X "$1" -H "Authorization: Bearer $TOKEN" -H "Accept: application/vnd.github+json" \
      ${3:+--data-binary "@$3"} "https://api.github.com/repos/$REPO$2"
  fi
}

# Accès vérifié tout de suite (jeton refusé, dépôt introuvable…), avec le message de GitHub.
if ! api GET "" > /dev/null; then
  echo "GitHub refuse l'accès au dépôt $REPO : vérifiez le jeton ou « gh auth status »." >&2
  exit 1
fi

if api GET "/releases/tags/v$V" >/dev/null 2>&1; then
  echo "La version v$V (fichier VERSION de la branche $(git rev-parse --abbrev-ref HEAD)) est déjà publiée : rien à faire."
  echo "Nouvelle version : récupérer d'abord les changements (ex. fusionner dev dans main, puis git pull) ;"
  echo "le fichier VERSION doit porter le numéro suivant."
  exit 0
fi

SHA="$(git rev-parse HEAD)"
if ! $DRY && [[ -z "$(git branch -r --contains "$SHA" 2>/dev/null)" ]]; then
  echo "Le commit $SHA n'est pas encore poussé : git push d'abord." >&2
  exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
api GET "/releases?per_page=100" > "$TMP/releases.json"

python3 - "$V" "$SHA" "$TMP" <<'PY'
import json, re, sys
v, sha, tmp = sys.argv[1:4]
key = lambda s: tuple(int(x) for x in re.findall(r"\d+", s))
releases = [r for r in json.load(open(f"{tmp}/releases.json"))
            if re.fullmatch(r"v\d+(\.\d+)*", r.get("tag_name", "")) and not r.get("draft")]
last = max((r["tag_name"][1:] for r in releases), key=key, default="0")
# Sections de CHANGELOG.md plus récentes que la dernière version publiée, jusqu'à celle-ci.
sections, cur = [], None
for line in open("CHANGELOG.md", encoding="utf-8"):
    m = re.match(r"^## (\d+(?:\.\d+)*)\s*$", line)
    if m:
        cur = m[1] if key(last) < key(m[1]) <= key(v) else None
        if cur:
            sections.append([cur, []])
        continue
    if cur:
        sections[-1][1].append(line)
parts = []
for num, lines in sections:
    text = "".join(lines).strip()
    parts.append(text if num == v else f"### Version {num}\n\n{text}")
installers = max((r for r in releases if r.get("assets")), key=lambda r: key(r["tag_name"]), default=None)
dl = (f"Pas d'installateurs pour cette version. Pour une première installation : ceux de la "
      f"[version {installers['tag_name'][1:]}]({installers['html_url']}), puis page **Mises à "
      f"jour** de l'interface, qui installe la version {v}.") if installers else \
     "Pas d'installateurs pour cette version."
body = "## Nouveautés\n\n" + "\n\n".join(parts) + "\n\n## Télécharger\n\n" + dl + "\n"
open(f"{tmp}/notes.md", "w").write(body)
json.dump({"tag_name": f"v{v}", "target_commitish": sha, "name": f"Auto-montage {v}",
           "body": body, "prerelease": True}, open(f"{tmp}/release.json", "w"), ensure_ascii=False)
PY

if $DRY; then
  cat "$TMP/notes.md"
  echo "(essai : rien n'est publié ; commit $SHA)"
  exit 0
fi
if ! api POST "/releases" "$TMP/release.json" > "$TMP/created.json"; then
  echo "GitHub a refusé la publication (droits du jeton ou de « gh » ?)." >&2
  exit 1
fi
python3 -c 'import json,sys; print("Publiée :", json.load(open(sys.argv[1]))["html_url"])' "$TMP/created.json"
