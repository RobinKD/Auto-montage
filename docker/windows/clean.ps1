# Libère la place prise par Docker : anciennes versions de l'image d'Auto-montage (une par mise
# à jour), images sans nom et cache de construction. Garde l'image en service et les volumes
# (connexion à Claude, Whisper et ses modèles, paquets Node), et tout le dépôt.
. "$PSScriptRoot\common.ps1"
Wait-Docker
Write-Host 'Place prise par Docker :'
docker system df
Write-Host ''
Write-Host "Le nettoyage supprime les anciennes versions de l'image d'Auto-montage, les images sans nom"
Write-Host 'et le cache de construction (les prochaines constructions d''images seront un peu plus longues).'
Write-Host "Il garde l'image en service, la connexion à Claude, Whisper, vos rushes et vos montages."
$answer = Read-Host 'Nettoyer ? [o/N]'
if ($answer -notmatch '^(o|oui)$') { Write-Host "Rien n'a été supprimé."; Wait-Close; exit 0 }
docker image prune -f
docker builder prune -f
Write-Host ''
Write-Host 'Après le nettoyage :'
docker system df
Write-Host 'Terminé. Docker Desktop peut mettre quelques minutes à rendre la place au disque de l''ordinateur.'
Wait-Close
