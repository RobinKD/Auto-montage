#!/bin/bash
# Libère la place prise par Docker (ou Podman) : anciennes versions de l'image d'Auto-montage
# (une par mise à jour), images sans nom et cache de construction. Garde l'image en service et
# les volumes (connexion à Claude, Whisper et ses modèles, paquets Node), et tout le dépôt.
. "$(dirname "$0")/lib.sh"
am_require_engine
echo "Place prise par $(am_engine_name) :"
"$AM_ENGINE" system df || true
echo
echo "Le nettoyage supprime les anciennes versions de l'image d'Auto-montage, les images sans nom"
echo "et le cache de construction (les prochaines constructions d'images seront un peu plus longues)."
echo "Il garde l'image en service, la connexion à Claude, Whisper, vos rushes et vos montages."
read -r -p "Nettoyer ? [o/N] " answer || answer=""
case "$answer" in
  o|O|oui|Oui|OUI) ;;
  *) am_pause "Rien n'a été supprimé."; exit 0 ;;
esac
"$AM_ENGINE" image prune -f
"$AM_ENGINE" builder prune -f 2>/dev/null || true
echo
echo "Après le nettoyage :"
"$AM_ENGINE" system df || true
am_pause "Terminé. Docker Desktop peut mettre quelques minutes à rendre la place au disque de l'ordinateur."
