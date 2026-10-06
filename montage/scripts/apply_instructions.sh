#!/bin/bash
# Applique les consignes de montage (work/instructions.md) au rush en cours avec Claude Code,
# sans interaction : Claude lit la transcription et écrit work/suggestions.json (moments à
# garder, avec raisons) et work/edit_choices.json (coupes, corrections, voix modifiée, caisse,
# pastilles, intro), que make_moments.py et build_edit.py utilisent ensuite.
# Usage : bash scripts/apply_instructions.sh   (Claude Code connecté : « claude » une fois, ou
# ANTHROPIC_API_KEY). Lancé par la préparation de la page « Rushes et montages » quand on le
# demande. Éléments de style : sons personnels (public/sfx/perso/) et planches des vidéos
# d'exemple (work/style/videos/*.planche.jpg) ; analyse écrite dans work/analyse_style.md.
set -euo pipefail
cd "$(dirname "$0")/.."
HAS_TEXT=false; [ -s work/instructions.md ] && HAS_TEXT=true
SOUNDS="$(ls public/sfx/perso/*.wav 2>/dev/null | sed 's|.*/||; s|\.wav$||' | sed 's|^|perso-|' | paste -sd ' ' - || true)"
SHEETS="$(ls work/style/videos/*.planche.jpg 2>/dev/null | sed 's|^|montage/|' | paste -sd ' ' - || true)"
if ! $HAS_TEXT && [ -z "$SOUNDS" ] && [ -z "$SHEETS" ]; then
  echo "Pas de consignes (work/instructions.md) ni d'éléments de style (work/style/)." >&2; exit 1
fi
[ -s work/segments.json ] || { echo "Rush pas encore transcrit (work/segments.json)." >&2; exit 1; }
command -v claude >/dev/null || { echo "Claude Code n'est pas installé." >&2; exit 1; }
RUSH="$(basename "$(cat work/rush.txt)")"
LANGUE="$(python3 -c "import sys; sys.path.insert(0, 'scripts'); import moments_lib as m; print(m.LANGUAGES[m.language('work', 'langue_rush.txt')].lower())")"

CONSIGNES="Applique les consignes de montage de montage/work/instructions.md"
$HAS_TEXT || CONSIGNES="Il n'y a pas de consignes écrites : fais un montage soigné (doublons et ratés retirés) en t'inspirant des éléments de style ci-dessous"
STYLE=""
if [ -n "$SOUNDS" ]; then
  STYLE="$STYLE
Sons personnels fournis par l'utilisateur (effets utilisables) : $SOUNDS. Place-les là où ils servent le propos ou imitent le style des exemples, avec \"sfx\": [[segment, mot, \"perso-<nom>\"], …] dans edit_choices.json (le son part au début du mot)."
fi
if [ -s work/style/analyse/resultat.json ]; then
  # Analyse détaillée déjà faite (bouton « Analyser les vidéos d'exemple ») : elle sert de guide.
  STYLE="$STYLE
Les vidéos d'exemple ont déjà été analysées : lis montage/work/style/analyse/resultat.json (éléments repérés, reproductibles ou non, style de sous-titres proposé) et applique ce qui est reproductible (coupes, intro, pastilles, sons, et le style de sous-titres proposé dans montage/work/caption_style.json s'il n'existe pas encore)."
elif [ -n "$SHEETS" ]; then
  STYLE="$STYLE
Vidéos d'exemple fournies pour le style : regarde leurs planches (12 images chacune, lis-les avec Read) : $SHEETS. Repère le rythme des coupes, les textes à l'écran (intro tapée, pastilles), la place et le style des sous-titres, les effets. Reproduis ce que le montage sait faire (coupes, intro, pastilles, caisse, voix modifiée, sons personnels) et écris dans montage/work/analyse_style.md, en quelques lignes, ce que tu as vu et ce qui n'a pas pu être reproduit automatiquement (police, effet visuel particulier…), pour que l'utilisateur le voie."
fi
HIDDEN="$(python3 -c "import sys; sys.path.insert(0, 'scripts'); from moments_lib import hidden_effects; print(', '.join(hidden_effects('work/effets_page.json')))" 2>/dev/null || true)"
if [ -n "$HIDDEN" ]; then
  STYLE="$STYLE
Effets écartés par l'utilisateur (page « Rushes et montages ») : $HIDDEN. Ne les utilise pas, sauf si les consignes les demandent explicitement (cash = caisse, bop = pastille sonore, clavier, voix = voix modifiée, gainvoix = volume de la voix par moment (sans effet sur toi), typed = texte tapé, chip = pastille)."
fi
PROMPT="Tu travailles dans le projet Auto-montage (interface locale : ne publie aucune page claude.ai, ne lance aucun rendu).
Le rush « $RUSH » vient d'être préparé ; sa transcription par segment est dans montage/work/segments.json. On y parle $LANGUE : les sous-titres corrigés, l'intro et les pastilles sont dans cette langue, sauf si les consignes en demandent une autre.
$CONSIGNES, comme aux étapes 3 et 5 de CLAUDE.md pour un nouveau rush :$STYLE
1. Écris montage/work/suggestions.json : {\"keep\": [ids des segments à garder], \"reasons\": {\"id\": \"raison du retrait\"}} (doublon : garder la 2e prise ; ratés, apartés retirés).
2. Écris montage/work/edit_choices.json au format décrit en tête de montage/scripts/build_edit.py (près de CHOICES_PATH), avec \"rush\": \"$RUSH\" : coupes au mot près (keep), corrections de sous-titres (fixes), voix modifiée (funny : [segment, premier mot, dernier mot, voix] avec voix = tremblement, robot, ecureuil, lutin, geant, megaphone, telephone, salle, batterie, radio ou choeur), bruit de caisse (cash), pastilles (chips), texte de l'intro (intro), sons (sfx : sons par défaut whoosh, pop, ding, boum, montee, glitch, photo, faux, juste, bop, cash, ou personnels), effets visuels (vfx : zoom avant, zoom sec, dézoom, secousse, flash, mot mis en valeur), selon les consignes, avec mesure (quelques effets bien placés, pas sur chaque phrase). Les réglages de tout le montage (zooms automatiques, sous-titres qui rebondissent, mot prononcé en couleur) vont dans montage/work/habillage.json : {\"autoZoom\": 0 à 1.6, \"pop\": vrai/faux, \"karaoke\": vrai/faux, \"karaokeColor\": \"#rrggbb\"}.
3. Si les consignes parlent du style des sous-titres (police, taille, gras, couleur, majuscules, nombre de lignes, taille des lignes entre elles), écris montage/work/caption_style.json au format décrit en tête de montage/scripts/fonts_lib.py ; « cd montage && python3 scripts/fonts_lib.py » liste les polices utilisables (identifiants) et le style actuel.
4. Vérifie que « cd montage && python3 scripts/build_edit.py » passe ; corrige tes fichiers sinon.
Ne modifie aucun autre fichier (sauf montage/work/analyse_style.md, montage/work/caption_style.json et montage/work/habillage.json). Termine par un résumé de deux ou trois lignes."

cd ..
# Écriture limitée à montage/work/ (pas de mode acceptEdits, qui accepterait toute
# modification du projet) : un texte piégé dans le rush ou les consignes ne peut pas faire
# réécrire un script que Claude lancerait ensuite.
claude -p "$PROMPT" \
  --allowedTools "Read" "Write(montage/work/**)" "Edit(montage/work/**)" "Glob" "Grep" "Bash(cd montage && python3 scripts/build_edit.py)" "Bash(python3 scripts/build_edit.py)" "Bash(cd montage && python3 scripts/fonts_lib.py)" "Bash(python3 scripts/fonts_lib.py)" \
  --disallowedTools "Edit(montage/work/chat/**)" "Edit(montage/work/**/*.html)" "Edit(montage/work/**/*.js)" "Write(montage/work/chat/**)" "Write(montage/work/**/*.html)" "Write(montage/work/**/*.js)"  # comme CHAT_DENIED (local_server.py)
