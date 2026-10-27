# Installateurs

| Système | Format | Script | Installé dans |
| --- | --- | --- | --- |
| Linux x86_64 | AppImage | `linux/build-appimage.sh` (appimagetool) | `~/Auto-montage` |
| Windows | `.exe` (Inno Setup 6) | `windows/auto-montage.iss` | `Documents\Auto-montage` |
| macOS | `.dmg` contenant `Auto-montage.app` | `macos/build-dmg.sh` (à lancer sur un Mac) | `~/Auto-montage` |

Chaque installateur embarque une copie du projet (`make-payload.sh <sortie> [version]
[programme]` : fichiers suivis par git, fins de ligne de `.gitattributes`, fichier `VERSION`) et
le programme compilé pour son système (`.github/workflows/programme.yml`), placé dans
`montage/bin/auto-montage` (`auto-montage.exe` sous Windows). Au premier lancement, ou quand la
version change, elle est copiée dans le dossier d'installation par-dessus l'ancienne : les rushes,
rendus, fichiers de travail et sélections (non versionnés) restent.

Les raccourcis lancent le programme : `auto-montage start [downloads/]` installe ce qui manque au
premier lancement (Node, paquets npm, ffmpeg, navigateur du rendu, whisper.cpp, modèles, police,
Claude Code : 10 à 20 min), sert l'interface sur 127.0.0.1 et l'ouvre dans le navigateur ; il reste
ouvert dans sa fenêtre de terminal tant que l'interface tourne (fermer la fenêtre l'arrête).
`auto-montage stop` l'arrête. Claude Code : celui de la machine, sinon
`montage/claude/node_modules/.bin/claude` (avec Node de `montage/node`).

- **Linux** : `linux/AppRun` installe le projet, crée l'entrée du menu (avec ses actions) qui
  pointe vers l'AppImage, puis lance `auto-montage start` dans une fenêtre de terminal (gardée
  ouverte après une erreur). `Auto-montage.AppImage [open|downloads|rushes|claude|stop|install]`.
  Programme Linux x86_64 seulement.
- **Windows** : installation pour l'utilisateur seul (pas de droits administrateur), raccourcis
  bureau et menu Démarrer (Auto-montage, Téléchargements, Dossier des rushes, Claude Code,
  Arrêter) ; à la fin de l'installation, case « Lancer Auto-montage ».
- **macOS** : application AppleScript (`macos/main.applescript`) ; `macos/first-run.sh` copie le
  projet, puis l'application propose les actions, lancées dans le Terminal. Programme universel
  (`lipo -create` des programmes `macos-arm64` et `macos-x86_64`, signé « ad hoc » :
  obligatoire sur Apple Silicon). Signature « ad hoc » seulement.

## Publier une version

Passer `VERSION` au numéro suivant (ex. `0.2` → `0.3`), ajouter en tête de `CHANGELOG.md` la
section `## 0.3`, pousser sur main, puis pousser le tag de la version :

    git tag v0.3 && git push origin v0.3

GitHub Actions (`.github/workflows/installateurs.yml`) construit et vérifie alors les trois
installateurs, puis publie la version `v0.3` avec eux (toujours en pré-version). Notes : les sections de `CHANGELOG.md` depuis la dernière version publiée
(`notes-version.sh --installateurs`), puis `release-notes.md`. Le tag doit être posé sur un
commit dont `VERSION` porte ce numéro, sinon rien n'est publié. Un numéro déjà publié avec ses
installateurs n'est jamais refait ; une version publiée sans installateurs (voir ci-dessous) les
reçoit. Les installateurs ne sont construits que pour une version. Pour essayer une modification de
l'empaquetage : lancement à la main sur une branche (onglet Actions > Installateurs > Run workflow) : construction et vérification seulement
(onglet Actions > Artifacts).

Sans GitHub Actions, la version peut être publiée sans installateurs :

    packaging/publish-release.sh           # --essai : affiche les notes sans rien publier

Le script crée la version `v0.3` sur GitHub (tag sur le commit poussé, toujours en
pré-version) avec les sections du changelog depuis la dernière version publiée, et
renvoie vers les installateurs de la dernière version qui en a. Accès : commande `gh` connectée,
ou jeton dans `GH_TOKEN` (droit « Contents : Read and write »). Les installations existantes se
mettent à jour par la page « Mises à jour », qui n'a besoin que du tag.

Sans `gh` ni jeton : `packaging/notes-version.sh` affiche les notes de la version (et les
écrit dans `dist/notes-v<numéro>.md`), d'après les tags déjà publiés que git récupère ; on les colle
dans « Draft a new release » sur github.com (tag `v<numéro>` sur `main`, pré-version).

Sans GitHub Actions ni Mac : `macos/build-dmg-linux.sh <payload> <sortie> [programme macOS…]`
construit le .dmg sur Linux (application qui lance `main.applescript` par osascript, ISO
compressée par l'outil `dmg` de libdmg-hfsplus : voir l'en-tête du script). Le programme : un
seul fichier (universel, ou `auto-montage-macos-arm64` seul), ou plusieurs réunis par `llvm-lipo`
s'il est installé ; il doit avoir été compilé sur un Mac (déjà signé « ad hoc »).

Les installateurs ne sont pas signés : signer demanderait un compte Apple Developer (macOS) et
un certificat de signature de code (Windows).
