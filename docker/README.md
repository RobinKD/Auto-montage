# Auto-montage en local (Docker)

Le conteneur contient tout ce qu'il faut pour monter les rushes sur sa propre machine :
Claude Code (avec le plugin HyperFrames du projet), HyperFrames et son navigateur de
rendu, Python (ffmpeg, OpenCV), Whisper et ses modèles, la police Oliver, et une
interface web locale : la page des moments (version de travail, moments, corrections de
sous-titres, effets, découpes, variantes) et la page des téléchargements.

## Installation

Le plus simple : l'installateur de votre système, dans les
[versions publiées](https://github.com/RobinKD/Auto-montage/releases) (voir le README du dépôt).
Ce qui suit concerne l'utilisation depuis un clone du dépôt.

### Docker ou Podman (Linux)

Les lanceurs (`docker/*.sh`, `docker/lib.sh`) prennent le moteur qui répond, Docker d'abord,
sinon Podman ; `AM_ENGINE=podman` (ou `docker`) force le choix. Avec Podman, il faut aussi
`podman-compose` (ou `podman compose` avec un fournisseur compose). Podman sans root
fonctionne tel quel : root dans le conteneur y correspond à l'utilisateur de la machine, les
fichiers créés dans le dépôt lui appartiennent donc.


Prérequis : Docker Desktop (macOS, Windows) ou Docker avec le plugin Compose (Linux,
`docker compose version`). Docker Desktop : environ 8 Go de mémoire (Réglages > Resources).

```bash
git clone https://github.com/RobinKD/Auto-montage.git
cd Auto-montage
```

| Système | Installer le raccourci | Lanceurs |
| --- | --- | --- |
| Linux | `./docker/install-desktop.sh` | « Auto-montage » dans le menu et sur le bureau ; clic droit : Téléchargements, Claude Code, Terminal du conteneur, Nettoyer Docker, Arrêter |
| macOS | `./docker/install-macos.sh` | application « Auto-montage » (`~/Applications`, alias sur le bureau) avec un menu : moments, téléchargements, Claude Code, terminal du conteneur, nettoyage de Docker, arrêt |
| Windows | double-clic sur `docker\install-windows.bat` | « Auto-montage » sur le bureau ; menu Démarrer > Auto-montage : Téléchargements, Claude Code, Terminal du conteneur, Nettoyer Docker, Arrêter |

Les lanceurs démarrent Docker Desktop s'il ne tourne pas (macOS, Windows), puis le conteneur,
et ouvrent le navigateur sur <http://localhost:8080/>. Le premier lancement construit l'image
puis installe le reste (paquets Node, Whisper compilé avec ses modèles, environ 1,7 Go, police,
navigateur de rendu) : comptez 10 à 20 min, la fenêtre de terminal affiche l'avancement.
Les lancements suivants prennent quelques secondes.

Sans raccourci :

| | Linux, macOS | Windows (PowerShell) |
| --- | --- | --- |
| Interface | `docker/open.sh` | `docker\windows\open.ps1` |
| Téléchargements | `docker/open.sh downloads/` | `docker\windows\open.ps1 downloads/` |
| Claude Code | `docker/claude.sh` | `docker\windows\claude.ps1` |
| Terminal du conteneur | `docker/shell.sh` | `docker\windows\shell.ps1` |
| Nettoyer Docker | `docker/clean.sh` | `docker\windows\clean.ps1` |
| Arrêter | `docker/stop.sh` | `docker\windows\stop.ps1` |

« Nettoyer Docker » supprime les anciennes versions de l'image (une par mise à jour), les images
sans nom et le cache de construction ; il garde l'image en service et les volumes (connexion à
Claude, Whisper, paquets Node). Les lanceurs effacent déjà d'eux-mêmes l'ancienne image
d'Auto-montage après chaque reconstruction (`LABEL auto-montage` du Dockerfile). La place des
rushes et des montages se libère sur la page « Rushes et montages » (« Libérer de la place »).

Notes par système :

- **macOS** : au premier lancement de l'application, macOS demande d'autoriser le contrôle du
  Terminal : accepter. Sur Mac Apple Silicon, l'image est construite pour ARM (tout le projet
  existe en ARM Linux).
- **Windows** : cloner avec Git for Windows ; `.gitattributes` garde les scripts en fins de
  ligne Unix, sans quoi le conteneur refuse de démarrer (il l'explique dans le terminal). Les
  scripts PowerShell sont lancés avec `-ExecutionPolicy Bypass` par les raccourcis.

## Utilisation

1. **Connexion à Claude Code** (une fois) : page <http://localhost:8080/claude/> (« Ouvrir la
   page d'autorisation », « Autoriser » sur claude.ai, coller le code), ou lanceur « Claude
   Code ». Une clé d'API marche aussi : variable `ANTHROPIC_API_KEY` avant de
   lancer. La connexion est gardée dans le volume `claude-home`.
2. **Nouveau rush** : page <http://localhost:8080/rush/> (lien « Rushes et montages… » de la page
   des moments) : choisir la vidéo dans l'explorateur de fichiers, puis « Préparer ce rush ».
   Ou demander à Claude de suivre `CLAUDE.md`. Les scripts (`prepare.sh`, `render.sh`) lancés
   sur la machine sans les outils se relancent d'eux-mêmes dans le conteneur.
3. **Choisir les moments** sur <http://localhost:8080/> : mêmes fonctions que la page
   claude.ai. Les choix sont enregistrés dans `montage/work/local_db/`.
4. **« Générer la version de travail »** : le serveur local monte la variante affichée
   (sélection, montage, rendu 1080p, mise à jour de la page, environ 4 min) et la page se
   recharge toute seule. Pas besoin de Claude pour cette étape.
5. **Discuter avec Claude** (<http://localhost:8080/chat/>, lien sur les pages) : demander un
   changement de montage en français ; Claude lit le projet, modifie les choix
   (`montage/work/`) et lance les scripts de montage (sélection, montage, rendu 1080p, page).
   Sans personne pour valider ses actions, il n'a pas d'autres outils : pour le reste, lanceur
   « Claude Code ». Conversation gardée dans `montage/work/chat/`.
6. **Rendu 4K** une fois validé : « Télécharger » > « Créer la version 4K » (page des moments ou
   des téléchargements : `make_hd.py` puis `render.sh final`, avec barre d'avancement), ou
   `./scripts/render.sh final` dans le terminal du conteneur.

## Fonctionnement

- Le dépôt est monté dans le conteneur (`/app`) : rushes, rendus (`montage/out/`) et fichiers
  de travail (`montage/work/`) restent sur la machine. Les paquets Node et Whisper sont dans
  des volumes Docker (`node-modules`, `whisper`) : plus rapide sous macOS et Windows, où le
  dossier partagé est lent.
- Le conteneur démarre en root, donne les volumes à l'utilisateur `AM_UID`/`AM_GID` (celui de
  la machine sous Linux, fourni par les lanceurs) et continue sous son identité : les fichiers
  créés dans le dépôt lui appartiennent.
- Au démarrage, `docker/entrypoint.sh` lance `montage/scripts/setup.sh` (le même script que
  les sessions Claude Code sur le web ; chaque étape saute ce qui existe déjà), installe les
  plugins Claude Code, puis `montage/scripts/local_server.py` sur le port 8080.
- Les pages sont celles publiées sur claude.ai ; `montage/local/shim.js` leur fournit en local
  l'enregistrement (`window.claude.use("db")`) et le téléchargement.
- L'interface n'écoute que sur la machine (`127.0.0.1`). Autre port : variable `AM_PORT`.
  Elle ne répond qu'aux adresses `localhost`, `127.0.0.1` et `[::1]` (protection contre les
  sites piégés) ; autre nom : variable `AM_ALLOWED_HOSTS` (noms séparés par des virgules).
- Mettre à jour Claude Code : `docker compose build --pull`, puis relancer.
- Tout réinstaller : `docker compose down -v` (supprime aussi la connexion Claude Code), puis relancer.

## En cas de problème

Page **Journal** (<http://localhost:8080/logs/>, lien sur toutes les pages) : le détail de chaque
tâche et du démarrage ; « Télécharger le rapport » donne un fichier à joindre à une demande
d'aide. Les fichiers sont dans `montage/work/logs/`.

| Symptôme | Cause et solution |
| --- | --- |
| `…/setup.sh: Permission denied` au démarrage | Image construite avec une ancienne version du dépôt. Les lanceurs reconstruisent l'image à chaque lancement (`--build`, quelques secondes grâce au cache) ; à la main : `docker compose up -d --build`. |
| « Le conteneur ne peut pas lire le dépôt monté » | Le conteneur tourne sous un autre utilisateur que le propriétaire du dépôt : passer par les lanceurs, ou `AM_UID=$(id -u) AM_GID=$(id -g) docker compose up -d`. Sous SELinux (Fedora, RHEL…), le dépôt est monté avec `:z` : `docker compose up -d --build --force-recreate`. |
| Dépôt sur un disque monté sans droit d'exécution (`noexec`), ou scripts sans bit exécutable | Pris en charge : les scripts sont lancés par `bash`, Node et Whisper sont dans des volumes Docker. |
| Le conteneur s'arrête pendant l'installation | `docker compose logs app` ; une coupure réseau pendant le téléchargement de Whisper ou du navigateur se règle en relançant. |
