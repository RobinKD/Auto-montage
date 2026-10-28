# Sécurité

Auto-montage tourne sur votre machine : l'interface (http://localhost:8080) n'écoute que sur
`127.0.0.1`, ne répond qu'aux adresses locales et refuse les requêtes envoyées par d'autres
sites. Vos rushes et montages restent dans `montage/work/`, jamais versionné.

Claude Code, quand il travaille sans vous demander de valider chaque action (discussion,
application des consignes, analyse du style), tourne directement sur la machine, sans
conteneur pour l'isoler : il est limité par la liste des outils qu'Auto-montage lui permet
(lecture du projet, écriture dans `montage/work/` seulement, commandes de montage précises),
pas par une séparation du reste de l'ordinateur.

## Signaler une faille

Merci de ne pas ouvrir de ticket public : utilisez « Report a vulnerability » dans l'onglet
**Security** du dépôt (signalement privé), avec les étapes pour reproduire le problème.
