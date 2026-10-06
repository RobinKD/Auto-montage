## Télécharger

| Système | Fichier |
| --- | --- |
| Windows 10/11 | `Auto-montage-…-Windows-installateur.exe` |
| macOS (Intel et Apple Silicon) | `Auto-montage-…-macOS.dmg` |
| Linux (x86_64) | `Auto-montage-…-x86_64.AppImage` |

Auto-montage fonctionne dans un conteneur : il faut **Docker Desktop** (Windows, macOS ; gratuit
pour un usage personnel) ou **Docker / Podman** (Linux). Le premier lancement installe le
reste (10 à 20 min, environ 5 Go) ; les suivants prennent quelques secondes.

### Windows

1. Lancer l'installateur. Windows affiche « Windows a protégé votre ordinateur » (l'installateur
   n'est pas signé) : cliquer sur **Informations complémentaires**, puis **Exécuter quand même**.
2. À la fin, laisser cochée « Vérifier Docker Desktop et lancer Auto-montage ». S'il manque,
   l'installateur propose d'installer Docker Desktop ; redémarrer ensuite l'ordinateur.
3. Lancer **Auto-montage** depuis le bureau. Les autres actions (téléchargements, dossier des
   rushes, Claude Code, arrêt) sont dans le menu Démarrer > Auto-montage.

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
3. Installer [Docker Desktop](https://www.docker.com/products/docker-desktop/) si l'application
   le demande, le lancer une fois, puis rouvrir Auto-montage.
4. Accepter que l'application contrôle le Terminal (il affiche l'avancement).

Le projet est copié dans `~/Auto-montage` (dossier personnel).

### Linux

1. Rendre le fichier exécutable : clic droit > Propriétés > Autoriser l'exécution, ou
   `chmod +x Auto-montage-*.AppImage`.
2. L'ouvrir. Il copie le projet dans `~/Auto-montage`, ajoute **Auto-montage** au menu des
   applications (clic droit : téléchargements, dossier des rushes, Claude Code, terminal, arrêt)
   et ouvre l'interface.
3. Docker ou Podman est nécessaire : par exemple `sudo apt install podman podman-compose`
   (Debian, Ubuntu) ou `sudo dnf install podman podman-compose` (Fedora). Podman est utilisé
   automatiquement si Docker n'est pas disponible.

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
choix des moments, génération de la version de travail, rendu 4K et téléchargements.
