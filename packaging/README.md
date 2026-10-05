# Installateurs

| Système | Format | Script | Installé dans |
| --- | --- | --- | --- |
| Linux x86_64 | AppImage | `linux/build-appimage.sh` (appimagetool) | `~/Auto-montage` |
| Windows | `.exe` (Inno Setup 6) | `windows/auto-montage.iss` | `Documents\Auto-montage` |
| macOS | `.dmg` contenant `Auto-montage.app` | `macos/build-dmg.sh` (à lancer sur un Mac) | `~/Auto-montage` |

Chaque installateur embarque une copie du projet (`make-payload.sh` : fichiers suivis par git,
fins de ligne de `.gitattributes`, fichier `VERSION`). Au premier lancement, ou quand la version
change, elle est copiée dans le dossier d'installation par-dessus l'ancienne : les rushes, rendus,
fichiers de travail et sélections (non versionnés) restent. Le lancement passe ensuite par les
lanceurs de `docker/` (Docker ou Podman, voir `docker/README.md`).

- **Linux** : `linux/AppRun` installe le projet, crée l'entrée du menu (avec ses actions) qui
  pointe vers l'AppImage, puis ouvre une fenêtre de terminal pour l'avancement.
  `Auto-montage.AppImage [open|downloads|rushes|claude|shell|stop|install]`.
- **Windows** : installation pour l'utilisateur seul (pas de droits administrateur), raccourcis
  bureau et menu Démarrer ; `docker\windows\first-run.ps1` propose ensuite d'installer Docker
  Desktop (`winget`) s'il manque.
- **macOS** : application AppleScript (`macos/main.applescript`) ; `macos/first-run.sh` copie le
  projet, puis l'application vérifie Docker Desktop et propose les actions. Signature « ad hoc »
  seulement.

## Publier une version

Sans GitHub Actions (dépôt privé, minutes gratuites épuisées) : passer `VERSION` au numéro
suivant (ex. `0.2` → `0.3`), ajouter en tête de `CHANGELOG.md` la section `## 0.3`, pousser, puis :

    packaging/publish-release.sh           # --essai : affiche les notes sans rien publier

Le script crée la version `v0.3` sur GitHub (tag sur le commit poussé, pré-version tant que le
numéro commence par 0) avec les sections du changelog depuis la dernière version publiée, et
renvoie vers les installateurs de la dernière version qui en a. Accès : commande `gh` connectée,
ou jeton dans `GH_TOKEN` (droit « Contents : Read and write »). Les installations existantes se
mettent à jour par la page « Mises à jour », qui n'a besoin que du tag.

Sans `gh` ni jeton : `packaging/notes-version.sh` affiche les notes de la version (et les
écrit dans `dist/notes-v<numéro>.md`), d'après les tags déjà publiés que git récupère ; on les colle
dans « Draft a new release » sur github.com (tag `v<numéro>` sur `main`, pré-version).

Installateurs : `.github/workflows/installateurs.yml`, lancé à la main (onglet Actions >
Installateurs > Run workflow). Sur une branche : construction et vérification des trois
installateurs (onglet Actions > Artifacts). Sur un tag `v*` pas encore publié : publication de
la version avec ses installateurs et les notes (`CHANGELOG.md` puis `release-notes.md`). Un
numéro déjà publié n'est jamais refait.

Sans GitHub Actions ni Mac : `macos/build-dmg-linux.sh <payload> <sortie>` construit le .dmg
sur Linux (application qui lance `main.applescript` par osascript, ISO compressée par l'outil
`dmg` de libdmg-hfsplus : voir l'en-tête du script).

Les installateurs ne sont pas signés : signer demanderait un compte Apple Developer (macOS) et
un certificat de signature de code (Windows).
