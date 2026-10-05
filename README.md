# Auto-montage

Utilisation de Claude Code avec HyperFrames pour monter des vidéos face caméra.

À partir d'un rush, le projet fait le dérush (blancs, ratés, doublons), les zooms, les
sous-titres, l'intro tapée au clavier, les bruitages et la voix modifiée. Une page web sert
à choisir les moments à garder, corriger les sous-titres, ajouter des effets, découper un
moment, puis à regarder et télécharger la version de travail (1080p) et la version finale (4K).

## Deux façons de l'utiliser

| | En local (installateur) | Claude Code sur le web (claude.ai) |
| --- | --- | --- |
| Installation | télécharger l'installateur de votre système (ci-dessous) | aucune : ouvrir le dépôt dans une session Claude Code |
| Page des moments | <http://localhost:8080/> | page claude.ai privée, lien donné par Claude |
| « Générer la version de travail » | le serveur local monte la version lui-même | réveille la session Claude (environ 1 min de délai) |
| Téléchargements | <http://localhost:8080/downloads/>, fichiers complets | page claude.ai (fichiers découpés en morceaux de 14 Mo) |
| Fichiers (rushes, rendus) | sur votre machine | dans la session cloud, perdus quand elle se ferme |

## Installation

Télécharger le fichier de votre système dans les
[versions publiées (Releases)](https://github.com/RobinKD/Auto-montage/releases) :

| Système | Fichier | Installation |
| --- | --- | --- |
| Windows 10/11 | `Auto-montage-…-Windows-installateur.exe` | lancer l'installateur ; il propose d'installer Docker Desktop s'il manque ; raccourci « Auto-montage » sur le bureau |
| macOS | `Auto-montage-…-macOS.dmg` | glisser Auto-montage dans Applications ; à la première ouverture, autoriser l'application (voir ci-dessous) ; installer [Docker Desktop](https://www.docker.com/products/docker-desktop/) |
| Linux (x86_64) | `Auto-montage-…-x86_64.AppImage` | rendre le fichier exécutable et l'ouvrir ; Docker ou Podman nécessaire |

Les installateurs ne sont pas signés : Windows affiche « Windows a protégé votre ordinateur »
(Informations complémentaires > Exécuter quand même) et macOS refuse la première ouverture
(« Apple n'a pas pu vérifier… » : Réglages Système > Confidentialité et sécurité > Ouvrir quand
même, ou dans le Terminal `xattr -dr com.apple.quarantine /Applications/Auto-montage.app`). Le
pas à pas est dans les notes de chaque version et dans le fichier LISEZ-MOI du `.dmg`.

Prérequis : un moteur de conteneurs, [Docker Desktop](https://www.docker.com/products/docker-desktop/)
sous Windows et macOS (gratuit pour un usage personnel), Docker ou Podman sous Linux (par
exemple `sudo apt install podman podman-compose` ou `sudo dnf install podman podman-compose`).
Prévoir environ 8 Go de mémoire pour Docker Desktop (Réglages > Resources) et 5 Go d'espace
disque (image 2 Go, Whisper 1,7 Go, paquets Node 0,5 Go), plus la place des rushes et des rendus.

Le projet est installé dans `~/Auto-montage` (Linux, macOS) ou `Documents\Auto-montage`
(Windows). Les rushes vont dans `montage/public/rushes/` (action « Dossier des rushes »).
Mises à jour : depuis la 0.20, la page « Mises à jour » de l'interface les détecte et les
installe en un clic (montages et réglages gardés, rien à réautoriser sur Mac) ; tant que le
dépôt est privé, elle demande une fois un jeton GitHub en lecture seule. On peut aussi
installer une nouvelle version par-dessus : rushes, rendus et sélections sont gardés.

Les actions (clic droit sur le raccourci sous Linux, menu de l'application sous macOS, menu
Démarrer > Auto-montage sous Windows) : moments et version de travail, téléchargements,
dossier des rushes, Claude Code, terminal du conteneur, arrêt.

### Depuis le dépôt (développement)

```bash
git clone https://github.com/RobinKD/Auto-montage.git
cd Auto-montage
./docker/install-desktop.sh      # Linux ; macOS : ./docker/install-macos.sh ;
                                 # Windows : docker\install-windows.bat
```

Sous Windows, cloner avec Git for Windows : `.gitattributes` garde les scripts au format
attendu par le conteneur. Construire les installateurs : voir [`packaging/README.md`](packaging/README.md).

**Premier lancement** : construction de l'image puis installation de HyperFrames et de son navigateur, de Whisper
(compilé avec ses modèles, environ 1,7 Go) et des polices, soit 10 à 20 min. Une fenêtre de
terminal montre l'avancement, puis le navigateur s'ouvre sur la page des moments. Les
lancements suivants prennent quelques secondes.

**Connexion à Claude Code** (une fois) : page « Connexion à Claude »
(<http://localhost:8080/claude/>, lien dans les avertissements) : ouvrir la page d'autorisation
(claude.ai, dans votre navigateur déjà connecté), « Autoriser », coller le code. Ou le lanceur
« Claude Code » (terminal). Une clé d'API marche aussi (variable `ANTHROPIC_API_KEY`).

## Monter une vidéo

1. **Choisir le rush et préparer** (en local) : page « Rushes et montages »
   (<http://localhost:8080/rush/>, ouverte d'office tant qu'aucun rush n'est prêt, ou lien
   « Rushes et montages… » de la page des moments). « Choisir un fichier… » ouvre l'explorateur de
   fichiers et copie la vidéo dans `montage/public/rushes/` ; « Préparer ce rush » lance la
   transcription, la découpe en moments et une première version de travail (environ la durée
   du rush, plus 3 à 5 min), avec l'avancement à l'écran, puis ouvre la page des moments.
   Sans consignes particulières, le premier montage garde tous les moments sauf les 1res prises.
   Consignes et style (facultatif) : écrire le montage voulu dans la zone de texte (ou choisir
   un fichier), ajouter des effets sonores (bibliothèque de sons, utilisable sur tous les
   montages) et des vidéos d'exemple dont Claude s'inspire. Si Claude Code est connecté
   (lanceur « Claude Code », une fois), il les applique pendant la préparation ; sinon, une
   bannière indique ce qui ne fonctionne pas.
2. **Avec Claude** (facultatif, ou sur claude.ai) : lui donner le rush (fichier ou lien de téléchargement)
   et les consignes de montage (`instructions.md`) ; il suit `CLAUDE.md` : suggestions de
   moments argumentées, intro, effets, coupes au mot près.
3. **Choisir** sur la page des moments : cocher les moments, corriger les sous-titres, ajouter
   des effets sonores (caisse, bop, clavier, voix modifiée) et visuels (texte tapé, pastille),
   couper un moment (curseur « Découpe » puis « Couper ici »), garder des plans sans parole, créer des variantes.
   Chaque moment se lit habillé comme dans le montage.
   Préparer un autre rush met le montage en cours de côté : la page « Rushes et montages » le rouvre
   plus tard tel quel (section « Montages », sans rien recalculer).
4. **Générer la version de travail** (bouton de la page), la regarder sur la page.
   En local, « Discuter avec Claude » (<http://localhost:8080/chat/>) permet aussi de lui
   demander les changements en français, sans terminal (Claude Code connecté une fois).
5. **Rendu final 4K** une fois validé : en local, « Télécharger » > « Créer la version 4K » (sur
   la page des moments ou des téléchargements ; long, l'avancement s'affiche) ; sinon le
   demander à Claude. Les versions se téléchargent depuis « Télécharger » ou la page des
   téléchargements.

## Fonctionnement

```
rush ──prepare.sh──> transcription par segment (Whisper) + visage + bruitages
     ──make_moments.py──> page des moments (extraits vidéo, habillage calculé dans la page)
page ──choix (sélection, corrections, effets, découpes)──> use_selection.py
     ──build_edit.py──> montage (clips, sous-titres, zooms, effets, voix : src/data/edit.json)
     ──render.sh apercu | final──> rendu HyperFrames : out/montage_apercu.mp4 (1080x1920), out/montage_4k.mp4
```

- `montage/` : scripts du montage et du rendu HyperFrames, gabarits des pages ; le détail de chaque étape
  est dans [`montage/README.md`](montage/README.md).
- `CLAUDE.md` : déroulé suivi par Claude pour un nouveau rush.
- `docker/` : image, lanceurs et raccourcis ; détails dans [`docker/README.md`](docker/README.md).
- `tests/` : test de bout en bout (rush de synthèse, préparation et génération par l'interface),
  lancé à la main par GitHub Actions (`.github/workflows/tests.yml`, onglet Actions) avec
  Docker et Podman, ou sur sa machine (`tests/e2e.sh`). Les machines Windows et macOS de GitHub ne font pas tourner de
  conteneurs Linux : on y vérifie les installateurs et les lanceurs (`installateurs.yml`).
- `packaging/` : installateurs (AppImage, .exe, .dmg) construits et publiés par GitHub Actions
  quand un tag de version `v*` est poussé (`.github/workflows/installateurs.yml`) ; voir
  [`packaging/README.md`](packaging/README.md).
- Non versionnés : rushes (`montage/public/rushes/`), fichiers de travail (`montage/work/`),
  rendus (`montage/out/`) et la police Oliver (usage personnel, téléchargée par
  `scripts/fetch_fonts.sh`).

## Licence

Auto-montage est distribué sous licence [GNU GPL version 3](LICENSE). La police Oliver n'est
pas fournie (usage personnel). Les extraits et captures du tutoriel ont leur propre licence
(CC BY-SA 4.0, Pixabay) : voir les fichiers `LICENCE.txt` de `montage/local/tutoriel/`.
