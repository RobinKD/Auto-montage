## Télécharger

| Système | Fichier |
| --- | --- |
| Windows 10/11 | `Auto-montage-…-Windows-installateur.exe` |
| macOS (Intel et Apple Silicon) | `Auto-montage-…-macOS.dmg` |
| Linux (x86_64) | `Auto-montage-…-x86_64.AppImage` |

Les installateurs contiennent le programme ; rien d'autre à installer avant. Le premier
lancement télécharge et installe le reste (10 à 20 min, environ 7 Go, connexion Internet
nécessaire) ; les suivants prennent quelques secondes. Auto-montage tourne dans une fenêtre de
terminal qui reste ouverte pendant qu'on l'utilise (la fermer l'arrête) et ouvre l'interface dans
le navigateur.

### Windows

1. Lancer l'installateur. Windows affiche « Windows a protégé votre ordinateur » (l'installateur
   n'est pas signé) : cliquer sur **Informations complémentaires**, puis **Exécuter quand même**.
2. À la fin, laisser cochée « Lancer Auto-montage » : une fenêtre montre l'installation du
   reste, puis le navigateur s'ouvre sur l'interface.
3. Les fois suivantes, lancer **Auto-montage** depuis le bureau. Les autres actions
   (téléchargements, dossier des rushes, Claude Code, arrêt) sont dans le menu Démarrer >
   Auto-montage.

Le projet est installé dans `Documents\Auto-montage` ; les rushes vont dans
`Documents\Auto-montage\montage\public\rushes` (raccourci « Dossier des rushes »).

### macOS

1. Ouvrir le `.dmg` et glisser **Auto-montage** dans **Applications** (le `.dmg` contient aussi
   un fichier « LISEZ-MOI - première ouverture » qui reprend ces étapes).
2. Au premier lancement, macOS bloque l'application (« Apple n'a pas pu vérifier que
   "Auto-montage" ne contient pas de logiciel malveillant ») : elle n'est pas notarisée par
   Apple (abonnement développeur payant). Cliquer sur **Terminé**, puis, au choix :
   - **Réglages Système > Confidentialité et sécurité**, partie **Sécurité** : **Ouvrir quand
     même**, mot de passe, puis rouvrir l'application et confirmer **Ouvrir quand même**. Le
     bouton n'apparaît que dans l'heure qui suit la tentative d'ouverture (sur macOS 14 et
     avant : clic droit sur l'application > Ouvrir) ;
   - ou, dans le **Terminal** (Applications > Utilitaires), coller cette ligne puis Entrée :
     `xattr -dr com.apple.quarantine /Applications/Auto-montage.app`, et ouvrir l'application.
   À refaire une seule fois par version.
3. Accepter que l'application contrôle le Terminal : Auto-montage y tourne et y affiche
   l'avancement.

Le projet est copié dans `~/Auto-montage` (dossier personnel).

### Linux

1. Rendre le fichier exécutable : clic droit > Propriétés > Autoriser l'exécution, ou
   `chmod +x Auto-montage-*.AppImage`.
2. L'ouvrir. Il copie le projet dans `~/Auto-montage`, ajoute **Auto-montage** au menu des
   applications (clic droit : téléchargements, dossier des rushes, Claude Code, arrêt) et
   lance Auto-montage dans une fenêtre de terminal, qui ouvre l'interface dans le navigateur.

Si l'AppImage ne s'ouvre pas (message sur FUSE) : `sudo apt install libfuse2` (Ubuntu 22.04 et
plus récent), ou lancer `./Auto-montage-*.AppImage --appimage-extract-and-run`.

## Mise à jour

Depuis la version 0.20 : page **Mises à jour** de l'interface (<http://localhost:8080/updates/>,
bandeau quand une version est disponible) : un clic installe la nouvelle version et Auto-montage
redémarre ; rien n'est à réinstaller (ni à réautoriser sur Mac). Si le programme a été modifié
et ne marche plus, « Réinitialiser Auto-montage » sur la même page le retélécharge (0.51).
On peut aussi installer la nouvelle version par-dessus : les rushes, rendus et sélections sont
conservés.

## Utilisation

Voir le [README](https://github.com/RobinKD/Auto-montage#readme) : préparation d'un rush,
choix des moments, génération de la vidéo entière, rendu 4K et téléchargements.
