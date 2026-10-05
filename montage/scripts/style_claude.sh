#!/usr/bin/env bash
# Analyse des vidéos d'exemple, seconde étape : Claude regarde les planches datées et les
# mesures de scripts/analyze_style.py (work/style/analyse/faits.json) et écrit ce qu'il a
# repéré, et ce qu'Auto-montage peut en reproduire, dans work/style/analyse/resultat.json
# (affiché sur la page « Rushes et montages ») et work/analyse_style.md (résumé lu à la
# préparation et montré sur la page des moments).
set -euo pipefail
cd "$(dirname "$0")/.."
[ -s work/style/analyse/faits.json ] || { echo "Mesures absentes : lancer d'abord scripts/analyze_style.py." >&2; exit 1; }
command -v claude >/dev/null || { echo "Claude Code n'est pas installé." >&2; exit 1; }
SHEETS="$(ls work/style/analyse/*/planche-*.jpg 2>/dev/null | sed 's|^|montage/|' | paste -sd ' ' - || true)"
rm -f work/style/analyse/resultat.json

PROMPT="Tu travailles dans le projet Auto-montage (montage automatique de vidéos face caméra, format vertical).
L'utilisateur a fourni des vidéos d'exemple dont il aime le style. Leurs mesures sont dans montage/work/style/analyse/faits.json (durée, instants des coupes, durée moyenne des plans, attaques sonores = bruitages probables, planches). Regarde toutes les planches (Read) : $SHEETS
Chaque image porte son instant dans la vidéo (« (coupe) » = juste après un changement de plan).

Repère tout ce qui fait le style : rythme des coupes, sous-titres (police, graisse, taille, couleur, majuscules, contour ou ombre, position, nombre de mots, animation mot par mot…), textes à l'écran (titres, intro, pastilles, listes), zooms et cadrage, transitions, effets visuels (couleurs, filtres, emojis, images ajoutées), sons (bruitages aux instants des attaques sonores, musique).

Ce qu'Auto-montage sait faire :
- coupes au mot près, rythme du montage (retirer blancs et hésitations) ;
- sous-titres sur 2 lignes équilibrées, en bas du visage : police (liste : « cd montage && python3 scripts/fonts_lib.py »), graisse, taille (36-120 px sur 1080), majuscules, couleur, ombre (intensité 0-100, diffusion du flou 0-60 px, décalage ox vers la droite et oy vers le bas en px, 40 px au plus) ; apparition avec rebond, mot prononcé en couleur (karaoké), mots mis en valeur (plus gros, en couleur) ; pas de contour, position fixe ;
- intro en texte tapé au clavier, pastilles (bulle blanche avec texte, son « bop ») ;
- zooms automatiques alternés sur le visage (aucun, légers, normaux, marqués), et sur un moment : zoom avant, zoom sec, dézoom, secousse, flash blanc ;
- sons fournis : caisse enregistreuse, bop, clavier, whoosh, pop, ding, boum, montée, glitch, déclencheur photo, faux (buzzer), juste (carillon) ; sons personnels ajoutés par l'utilisateur ; voix modifiée (tremblante) ;
- pas de transitions élaborées (glissés, volets), filtres de couleur, emojis, images ajoutées, musique de fond.

Écris montage/work/style/analyse/resultat.json, en français, exactement sous cette forme (JSON valide) :
{\"resume\": \"2 ou 3 phrases sur le style\",
 \"rythme\": \"le rythme mesuré (un plan toutes les X s, coupes…), en mots simples\",
 \"elements\": [{\"categorie\": \"Sous-titres|Rythme|Texte à l'écran|Zoom et cadrage|Transitions|Effets visuels|Sons\", \"observe\": \"ce que tu vois, précis\", \"ou\": \"vidéo et instants\", \"reproductible\": \"oui|en partie|non\", \"comment\": \"le réglage d'Auto-montage à utiliser, ou ce qui manque et ce que l'utilisateur peut faire (ex. ajouter un son « whoosh » comme son personnel)\"}],
 \"sousTitres\": null ou {\"font\": \"identifiant de police de fonts_lib.py\", \"weight\": 700, \"size\": 64, \"uppercase\": true, \"color\": \"#rrggbb\", \"shadow\": 50, \"blur\": 14, \"ox\": 0, \"oy\": 0} (le style de sous-titres le plus proche possible des exemples),
 \"aFaire\": [\"conseils concrets à l'utilisateur pour s'en approcher\"]}
Puis écris montage/work/analyse_style.md : quelques lignes, le résumé et ce qui ne peut pas être reproduit automatiquement.
L'utilisateur n'est pas développeur : dans « comment » et « aFaire », parle des boutons de l'interface (« Ajouter des effets sonores… » ou « Ajouter des sons… » pour un bruitage, « Style des sous-titres » sur la page des moments, les consignes de montage pour le rythme, l'intro et les pastilles), jamais de chemins de fichiers.
Ne modifie aucun autre fichier. Termine par une phrase de résumé."

cd ..
# Écriture limitée à montage/work/ (voir apply_instructions.sh).
claude -p "$PROMPT" \
  --allowedTools "Read" "Glob" "Write(montage/work/**)" "Edit(montage/work/**)" "Bash(cd montage && python3 scripts/fonts_lib.py)" "Bash(python3 scripts/fonts_lib.py)" \
  --disallowedTools "Read(montage/work/github_token)" "Edit(montage/work/chat/**)" "Edit(montage/work/github_token)" "Edit(montage/work/**/*.html)" "Edit(montage/work/**/*.js)" "Write(montage/work/chat/**)" "Write(montage/work/github_token)" "Write(montage/work/**/*.html)" "Write(montage/work/**/*.js)"  # comme CHAT_DENIED (local_server.py)
cd montage
python3 - <<'PY'
import json, sys
try:
    data = json.load(open("work/style/analyse/resultat.json"))
    assert isinstance(data.get("elements"), list)
except Exception as e:  # noqa: BLE001
    sys.exit(f"Claude n'a pas écrit une analyse lisible (work/style/analyse/resultat.json) : {e}")
print(f"Analyse écrite : {len(data['elements'])} éléments repérés")
PY
