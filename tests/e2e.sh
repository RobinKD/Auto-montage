#!/bin/bash
# Test de bout en bout de l'interface locale, par la même API que les boutons des pages :
# envoi d'un rush et d'un fichier de consignes, préparation, vérification des pages et des
# vidéos, puis « Générer la version de travail » avec une sélection modifiée.
# Usage : tests/e2e.sh <url de l'interface> <rush .mov> [--consignes-claude]
#   --consignes-claude : la préparation applique aussi les consignes avec Claude.
set -euo pipefail
URL="$1"
RUSH="$2"
WITH_CLAUDE=false
[ "${3:-}" = --consignes-claude ] && WITH_CLAUDE=true
NAME="essai.mov"
HERE="$(cd "$(dirname "$0")" && pwd)"
fail() { echo "ÉCHEC : $*" >&2; exit 1; }
json() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
# grep -q quitte à la première ligne trouvée : avec pipefail, curl ou echo reçoivent alors
# SIGPIPE et le test échouerait à tort. has lit toute l'entrée.
has() { grep "$@" >/dev/null; }

wait_job() {  # attend la fin de la tâche en cours, affiche les étapes
  local last="" state step t0=$SECONDS
  while true; do
    local job; job="$(curl -fsS "$URL/api/job")"
    state="$(echo "$job" | json 'd["state"]')"
    step="$(echo "$job" | json 'd["step"]')"
    [ "$step" != "$last" ] && echo "  [$((SECONDS - t0)) s] $step" && last="$step"
    # Barres de progression : opérations vues pendant la tâche (vérifiées après).
    label="$(echo "$job" | json '(d.get("progress") or {}).get("label", "")')"
    [ -n "$label" ] && case "$LABELS" in *"|$label|"*) ;; *) LABELS="${LABELS:-|}$label|" ; echo "    barre : $label" ;; esac
    case "$state" in
      done) return 0 ;;
      error) echo "$job" | json '"\n".join(d["log"][-30:])' >&2; fail "tâche en erreur : $(echo "$job" | json 'd["error"]')" ;;
    esac
    [ $((SECONDS - t0)) -lt 3600 ] || fail "tâche trop longue"
    sleep 5
  done
}

echo "1. Page d'accueil sans rush : renvoi vers « Rushes et montages »"
code="$(curl -s -o /dev/null -w '%{http_code} %{redirect_url}' "$URL/")"
case "$code" in "302 "*"/rush/") ;; *) fail "accueil : $code" ;; esac
curl -fsS "$URL/rush/" | has "Rushes et montages" || fail "page Rushes et montages"

echo "2. Envoi du rush et des consignes"
curl -fsS -X PUT --data-binary "@$RUSH" "$URL/api/rushes/$NAME" | json 'd["name"]' | has -x "$NAME" || fail "envoi du rush"
curl -fsS -X PUT -H "x-file-name: consignes.md" --data-binary "@$HERE/consignes-essai.md" "$URL/api/instructions" >/dev/null || fail "envoi des consignes"
curl -fsS "$URL/api/rushes" | json '[r["name"] for r in d["rushes"]], d["instructions"]["name"]' | has "$NAME" || fail "liste des rushes"
# Son personnel (bibliothèque) : un bip de 0,3 s généré ici.
BIP="$(mktemp -d)/Bip essai.wav"
python3 -c "
import math, struct, sys, wave
w = wave.open(sys.argv[1], 'wb'); w.setnchannels(1); w.setsampwidth(2); w.setframerate(48000)
w.writeframes(b''.join(struct.pack('<h', int(12000 * math.sin(2 * math.pi * 880 * i / 48000))) for i in range(14400)))
" "$BIP"
curl -fsS -X PUT --data-binary "@$BIP" "$URL/api/style/sons/Bip%20essai.wav" | has '"perso-bip-essai"' || fail "envoi d'un son personnel"

echo "3. Préparation (transcription, montage, rendu, page des moments), avec une pause"
MEM="$(curl -fsS "$URL/api/memoire?rush=$NAME")"
echo "$MEM" | json 'd["peak"] > 0 and d["peakOp"]' | has -x "Transcription\|Rendu de la vidéo" || fail "mémoire nécessaire : $MEM"
echo "  mémoire : $(echo "$MEM" | json '"%.1f Go au plus fort, %s" % (d["peak"] / 2**30, d["level"])')"
curl -fsS -X POST -H "content-type: application/json" \
  -d "{\"rush\": \"$NAME\", \"instructions\": $WITH_CLAUDE}" "$URL/api/prepare" >/dev/null || fail "lancement de la préparation"
# Pause dès la première barre de progression (version de travail ou transcription), puis
# reprise là où elle en était.
t0=$SECONDS
until label="$(curl -fsS "$URL/api/job" | json '(d.get("progress") or {}).get("label", "")')" && [ -n "$label" ]; do
  [ $((SECONDS - t0)) -lt 1800 ] || fail "préparation sans barre de progression"
  sleep 1
done
LABELS="|$label|"
curl -fsS -X POST "$URL/api/job/pause" >/dev/null || fail "mise en pause"
until curl -fsS "$URL/api/job" | json 'd["state"]' | has -x paused; do sleep 1; done
curl -fsS "$URL/api/job" | json 'd["paused"]["state"], d["paused"]["step"]' | has "paused" || fail "tâche en pause non décrite"
code="$(curl -s -o /dev/null -w '%{http_code}' -X POST -H "content-type: application/json" -d "{\"rush\": \"$NAME\"}" "$URL/api/prepare")"
[ "$code" = 409 ] || fail "nouvelle préparation acceptée pendant la pause ($code)"
echo "  en pause ($(curl -fsS "$URL/api/job" | json 'd["paused"]["current"]')), abandonnée en gardant tout"
curl -fsS "$URL/api/job/parts" | json '[p["id"] for p in d["parts"]]' | has "'" || fail "parties déjà faites non listées"
curl -fsS -X POST -H "content-type: application/json" -d '{"delete": []}' "$URL/api/job/abandon" >/dev/null || fail "abandon"
curl -fsS "$URL/api/job" | json 'd["paused"]' | has -x None || fail "tâche en pause restée après l'abandon"
curl -fsS "$URL/api/memoire?rush=$NAME" | json 'd["kept"]' | has "'" || fail "parties gardées non reprises"
curl -fsS -X POST -H "content-type: application/json" \
  -d "{\"rush\": \"$NAME\", \"instructions\": $WITH_CLAUDE}" "$URL/api/prepare" >/dev/null || fail "nouvelle préparation"
until label="$(curl -fsS "$URL/api/job" | json '(d.get("progress") or {}).get("label", "")')" && [ -n "$label" ]; do
  [ $((SECONDS - t0)) -lt 1800 ] || fail "préparation sans barre de progression"
  sleep 1
done
LABELS="$LABELS$label|"
curl -fsS -X POST "$URL/api/job/pause" >/dev/null || fail "deuxième mise en pause"
until curl -fsS "$URL/api/job" | json 'd["state"]' | has -x paused; do sleep 1; done
echo "  préparée à nouveau avec ce qui était gardé, en pause ($label), reprise"
curl -fsS -X POST "$URL/api/job/resume" >/dev/null || fail "reprise"
wait_job
curl -fsS "$URL/api/logs/rapport" | has "Parties gardées" || fail "parties gardées non signalées au journal"
# Mémoire mesurée (pas pour l'étape reprise en route : une partie était déjà faite).
curl -fsS "$URL/api/memoire?rush=$NAME" | json '[o["op"] for o in d["operations"] if o["measured"]]' | has "Rendu de la vidéo" \
  || fail "mémoire du rendu non mesurée"
curl -fsS "$URL/api/job" | json 'd["paused"]' | has -x None || fail "tâche en pause restée après la reprise"
case "$LABELS" in *Transcription*) ;; *) fail "pas de barre de progression « Transcription » (vu : ${LABELS:-aucune})" ;; esac

echo "4. Vérifications"
PAGE="$(curl -fsS "$URL/")"
echo "$PAGE" | has '"rush": "essai.mov"' || fail "page des moments du rush"
DATA="$(echo "$PAGE" | sed -n 's/^  const DATA = \(.*\);$/\1/p')"
N="$(echo "$DATA" | json 'len([m for m in d["moments"] if not m.get("gap")])')"
[ "$N" -ge 3 ] || fail "seulement $N moments de parole"
echo "  $N moments de parole"
echo "$DATA" | json '[c["id"] for c in d["sfxCatalog"]]' | has "perso-bip-essai" || fail "son personnel absent des effets de la page"
for f in /montage.mp4 /out/montage_apercu.mp4 "$(echo "$DATA" | json 'd["moments"][0]["clip"]' | sed 's|^|/|')"; do
  [ "$(curl -s -o /dev/null -w '%{http_code}' -H 'Range: bytes=0-99' "$URL$f")" = 206 ] || fail "vidéo $f"
done
curl -fsS "$URL/downloads/" | has "1080p" || fail "page des téléchargements"
curl -fsS "$URL/chat/" | has "Discuter\|Claude" || fail "page de discussion"
curl -fsS "$URL/logs/" | has "Journal" || fail "page Journal"
curl -fsS "$URL/tutoriel/" | has "Sans Claude Code" || fail "page Tutoriel"
[ "$(curl -s -o /dev/null -w '%{http_code}' "$URL/tutoriel/demo-effets.webm")" = 200 ] || fail "vidéo du tutoriel"
[ "$(curl -s -o /dev/null -w '%{http_code}' "$URL/tutoriel/voix/robot.mp3")" = 200 ] || fail "exemples des voix modifiées"
curl -fsS "$URL/api/status" | json 'd["tutorialSeen"]' | has -x False || fail "tutoriel pas proposé au premier lancement"
curl -fsS "$URL/api/logs" | json '[(l["kind"], l["state"]) for l in d["logs"]]' | has "('prepare', 'réussi')" \
  || fail "journal de la préparation absent ou pas « réussi »"
curl -fsS "$URL/api/logs" | json '[l["name"] for l in d["logs"]]' | has "demarrage.log" || fail "journal du démarrage absent"
curl -fsS "$URL/api/logs/rapport" | has "Rapport Auto-montage" || fail "rapport du journal"
curl -fsS "$URL/api/chat" | python3 -c "import json,sys; d=json.load(sys.stdin); assert \"messages\" in d and d[\"running\"] is False" \
  || fail "état de la discussion (/api/chat)"
# Style des sous-titres (police de la machine de rendu) et effets proposés.
FONT="$(curl -fsS "$URL/api/caption-style" | json '[f["id"] for f in d["fonts"]][-1]')"
curl -fsS -X PUT -H "content-type: application/json" -d "{\"font\": \"$FONT\", \"weight\": 700, \"size\": 70, \"uppercase\": false, \"color\": \"#ffffff\"}" \
  "$URL/api/caption-style" | json 'd["style"]["size"]' | has -x 70 || fail "style des sous-titres"
curl -fsS "$URL/api/effects" | json '[e["id"] for e in d["effects"] if e["kind"] != "perso" and e["shown"]]' | has -x "\[\]" \
  || fail "effets par défaut cochés sans réglage (ils doivent être décochés par défaut)"
curl -fsS -X PUT -H "content-type: application/json" -d '{"hidden": ["clavier"]}' "$URL/api/effects" >/dev/null || fail "effets proposés"
PAGE2="$(curl -fsS "$URL/")"
echo "$PAGE2" | has '"hiddenEffects": \["clavier"\]' || fail "effets écartés absents de la page des moments"
echo "$PAGE2" | has '"size": 70' || fail "style des sous-titres absent de la page des moments"
echo "  police des sous-titres : $FONT"
# Vidéo d'exemple : durée prévue de son analyse (l'analyse elle-même demande Claude).
curl -fsS -X PUT --data-binary "@$RUSH" "$URL/api/style/videos/exemple.mov" >/dev/null || fail "envoi d'une vidéo d'exemple"
curl -fsS "$URL/api/style/analyse" | json 'd["videos"], d["estimate"] > 0' | has "exemple.mov" || fail "analyse du style (estimation)"
D1="$(echo "$DATA" | json 'd["working"]["duration"]')"
echo "  version de travail : $D1 s"

echo "5. Générer la version de travail avec un moment de moins"
KEEP="$(echo "$DATA" | json 'json.dumps([m["id"] for m in d["moments"] if m["suggested"]][1:])')"
curl -fsS -X POST -H "content-type: application/json" \
  -d "{\"variant\": \"travail\", \"name\": \"Travail\", \"keep\": $KEEP}" "$URL/api/regen" >/dev/null || fail "lancement de la génération"
LABELS=""
wait_job
case "$LABELS" in *"Rendu de la vidéo"*) ;; *) fail "pas de barre de progression du rendu (vu : ${LABELS:-aucune})" ;; esac
D2="$(curl -fsS "$URL/" | sed -n 's/^  const DATA = \(.*\);$/\1/p' | json 'd["working"]["duration"]')"
echo "  nouvelle version : $D2 s"
python3 -c "import sys; sys.exit(0 if float('$D2') < float('$D1') else 1)" || fail "la version générée n'est pas plus courte ($D2 s contre $D1 s)"

echo "6. Montage mis de côté puis rouvert (montages enregistrés)"
post() { curl -fsS -X POST -H "content-type: application/json" -d "$2" "$URL$1"; }
post /api/projects/save '{}' | has '"ok": true' || fail "mise de côté du montage"
code="$(curl -s -o /dev/null -w '%{http_code}' "$URL/")"
[ "$code" = 302 ] || fail "accueil après mise de côté : $code au lieu du renvoi vers « Rushes et montages »"
PID="$(curl -fsS "$URL/api/projects" | json '[p["id"] for p in d["saved"] if p["rush"] == "essai.mov"][0]')"
post /api/projects/open "{\"id\": \"$PID\"}" | has '"ok": true' || fail "réouverture du montage"
D3="$(curl -fsS "$URL/" | sed -n 's/^  const DATA = \(.*\);$/\1/p' | json 'd["working"]["duration"]')"
[ "$D3" = "$D2" ] || fail "montage rouvert différent ($D3 s au lieu de $D2 s)"
[ "$(curl -s -o /dev/null -w '%{http_code}' -H 'Range: bytes=0-99' "$URL/out/montage_apercu.mp4")" = 206 ] || fail "rendu du montage rouvert"
echo "  rouvert : $D3 s"

echo "7. Deux vidéos pour un même rush (réglages différents : réencodées puis assemblées)"
SUITE="$(mktemp -d)/suite.mp4"
"${FFMPEG:-ffmpeg}" -v error -y -f lavfi -i testsrc2=size=720x1280:rate=25 -f lavfi -i "sine=f=300:r=44100" -t 4 \
  -c:v libx264 -pix_fmt yuv420p -c:a aac -ac 1 "$SUITE"
curl -fsS -X PUT --data-binary "@$SUITE" "$URL/api/rushes/suite.mp4" | has '"suite.mp4"' || fail "envoi de la 2e vidéo"
curl -fsS -X POST -H "content-type: application/json" \
  -d "{\"rushes\": [\"$NAME\", \"suite.mp4\"]}" "$URL/api/prepare" >/dev/null || fail "lancement de la préparation à deux vidéos"
LABELS=""
wait_job
case "$LABELS" in *"Assemblage des vidéos"*) ;; *) fail "pas de barre « Assemblage des vidéos » (vu : ${LABELS:-aucune})" ;; esac
curl -fsS "$URL/" | has '"rush": "essai (assemblage de 2).mov"' || fail "page des moments du rush assemblé"
curl -fsS "$URL/api/projects" | json '[p["rush"] for p in d["saved"]]' | has "essai.mov" || fail "montage d'essai.mov pas mis de côté"

echo "8. Fusionner deux moments, puis les séparer (page des moments)"
pdata() { curl -fsS "$URL/" | python3 -c "import json,re,sys; d=json.loads(re.search(r'^  const DATA = (\{.*\});$', sys.stdin.read(), re.M).group(1)); print($1)"; }
FIRST="$(pdata '[m["id"] for m in d["moments"] if not m.get("gap")][0] if sum(1 for m in d["moments"] if not m.get("gap")) > 1 else ""')"
if [ -n "$FIRST" ]; then
  curl -fsS -X POST -H "content-type: application/json" -d "{\"id\": $FIRST}" "$URL/api/moments/merge" >/dev/null || fail "fusion de deux moments"
  wait_job
  pdata '[m["id"] for m in d["moments"] if m.get("merged")]' | has -x "\[$FIRST\]" || fail "moment fusionné absent de la page"
  curl -fsS -X POST -H "content-type: application/json" -d "{\"id\": $FIRST}" "$URL/api/moments/unmerge" >/dev/null || fail "séparation du moment fusionné"
  wait_job
  pdata '[m["id"] for m in d["moments"] if m.get("merged")]' | has -x "\[\]" || fail "moment toujours fusionné après « Séparer »"
else
  echo "  (un seul moment de parole : étape passée)"
fi

echo "Tout est bon."
