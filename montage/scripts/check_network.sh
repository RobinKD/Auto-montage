#!/usr/bin/env bash
# Vérifie que les domaines de env/allowed-domains.txt sont joignables.
# Code de sortie 1 si au moins un domaine est bloqué par l'environnement.
cd "$(dirname "$0")/.."
blocked=()
while read -r domain url _; do
  [[ -z "$domain" || "$domain" == \#* ]] && continue
  code=$(curl -s -o /dev/null -m 10 -w "%{http_code}" "$url" 2>/dev/null)
  if [[ "$code" == "000" ]]; then
    echo "BLOQUÉ  $domain"
    blocked+=("$domain")
  else
    echo "ok      $domain"
  fi
done < env/allowed-domains.txt
if (( ${#blocked[@]} )); then
  echo
  echo "À ajouter dans Network access de l'environnement : ${blocked[*]}"
  exit 1
fi
