# Auto-montage
 
Utilisation de Claude Code avec HyperFrames pour monter des vidéos face caméra.
 
À partir d'un rush, le projet fait le dérush (blancs, ratés, doublons), les zooms, les
sous-titres, l'intro tapée au clavier, les bruitages et la voix modifiée. Une page web sert
à choisir les moments à garder, corriger les sous-titres, ajouter des effets, découper un
moment, puis à regarder et télécharger la vidéo entière (1080p) et la version finale (4K).
 
## Deux façons de l'utiliser
 
| | En local (installateur) | Claude Code sur le web (claude.ai) |
| --- | --- | --- |
| Installation | télécharger l'installateur de votre système (ci-dessous) | rien à installer : votre copie (fork) du dépôt sur GitHub, ouverte dans Claude Code ([pas à pas](#sur-claudeai-claude-code-sur-le-web)) |
| Page des moments | <http://localhost:8080/> | page claude.ai privée, lien donné par Claude |
| « Générer la vidéo entière » | le serveur local monte la version lui-même | réveille la session Claude (environ 1 min de délai) |
| Téléchargements | <http://localhost:8080/downloads/>, fichiers complets | page claude.ai (fichiers découpés en morceaux de 14 Mo) |
| Fichiers (rushes, rendus) | sur votre machine | dans la session cloud, perdus quand elle se ferme |
 
## Installer

Télécharger le fichier de votre système dans les
[versions publiées (Releases)](https://github.com/RobinKD/Auto-montage/releases) :

| Système | Fichier | Installation |
| --- | --- | --- |
| Windows 10/11 | `Auto-montage-…-Windows-installateur.exe` | lancer l'installateur ; raccourci « Auto-montage » sur le bureau |
| macOS | `Auto-montage-…-macOS.dmg` | glisser Auto-montage dans Applications ; à la première ouverture, autoriser l'application (voir ci-dessous) |
| Linux (x86_64) | `Auto-montage-…-x86_64.AppImage` | rendre le fichier exécutable et l'ouvrir |

Auto-montage s'installe directement sur l'ordinateur, sans autre logiciel à installer avant. Les installateurs ne sont pas signés : Windows affiche « Windows a protégé
votre ordinateur » (Informations complémentaires > Exécuter quand même) et macOS refuse la
première ouverture (« Apple n'a pas pu vérifier… » : Réglages Système > Confidentialité et
sécurité > Ouvrir quand même, ou dans le Terminal
`xattr -dr com.apple.quarantine /Applications/Auto-montage.app`). Le pas à pas est dans les
notes de chaque version.

**Premier lancement** : Auto-montage télécharge ce qui lui manque (Node.js s'il n'y en a pas de
version 20 ou plus, paquets de HyperFrames et son navigateur de rendu, ffmpeg, Parakeet, Whisper et leurs
modèles, environ 3,4 Go, polices, modèles des visages, Claude Code), soit 10 à 20 min. Une
fenêtre de terminal montre l'avancement (aussi écrit dans `montage/work/logs/demarrage.log`),
puis le navigateur s'ouvre sur l'interface (<http://localhost:8080/>). Les lancements suivants
prennent quelques secondes. Prévoir environ 8 Go de mémoire et 8 Go d'espace disque, plus la
place des rushes et des rendus.

Les rushes vont dans `montage/public/rushes/` du dossier d'installation (la page « Rushes et
montages » les y copie). Mises à jour : la page « Mises à jour » de l'interface les détecte et
les installe en un clic : nouveaux fichiers et programme compilé pour votre système, puis
Auto-montage se relance (montages et réglages gardés, rien à réautoriser sur Mac). Elle peut
aussi réinitialiser le programme : si Claude Code lancé dans le dossier a modifié des fichiers
et cassé quelque chose, « Réinitialiser Auto-montage » les retélécharge depuis GitHub (données
de l'utilisateur gardées). On peut aussi installer une nouvelle version par-dessus : rushes,
rendus et sélections sont gardés.

**Connexion à Claude Code** (une fois) : page « Connexion à Claude »
(<http://localhost:8080/claude/>, lien dans les avertissements) : ouvrir la page d'autorisation
(claude.ai, dans votre navigateur déjà connecté), « Autoriser », coller le code. Une clé d'API
marche aussi (variable `ANTHROPIC_API_KEY`).

### Depuis le dépôt (développement)

Il faut [Rust](https://rustup.rs) (la première compilation prend quelques minutes) :

```bash
git clone https://github.com/RobinKD/Auto-montage.git
cd Auto-montage
montage/am start                 # Windows : montage\am.cmd start
```

`start` compile le programme, installe ce qui manque (comme au premier lancement ci-dessus),
puis lance l'interface sur <http://127.0.0.1:8080/> et ouvre le navigateur (déjà lancée : ouvre
seulement la page) ; `montage/am stop` l'arrête. Construire les installateurs : voir
[`packaging/README.md`](packaging/README.md).

### Sur claude.ai (Claude Code sur le web)
 
Claude Code sur le web ouvre un dépôt GitHub de votre compte. Le plus simple est d'en faire une
copie (fork), une fois :
 
1. Créer un compte sur [GitHub](https://github.com/signup) si besoin (gratuit).
2. Sur <https://github.com/RobinKD/Auto-montage>, bouton **Fork** > « Create fork » : la copie
   `<votre-compte>/Auto-montage` est à vous ; aucun accès au compte d'Auto-montage n'est nécessaire.
3. Sur [claude.ai/code](https://claude.ai/code), connecter votre compte GitHub et installer
   l'application Claude sur cette copie (« Only select repositories » > Auto-montage).
4. Créer un environnement avec un accès réseau qui autorise les domaines de
   [`montage/env/allowed-domains.txt`](montage/env/allowed-domains.txt) (modèles de transcription,
   polices, HyperFrames…).
5. Ouvrir une session sur votre copie, puis donner à Claude le rush (fichier ou lien de
   téléchargement) et vos consignes : il suit `CLAUDE.md`.
Nouvelle version d'Auto-montage : sur la page de votre copie, **Sync fork** > « Update branch »,
puis ouvrir une nouvelle session.
 
## Monter une vidéo
 
1. **Choisir le rush et préparer** (en local) : page « Rushes et montages »
   (<http://localhost:8080/rush/>, ouverte d'office tant qu'aucun rush n'est prêt, ou lien
   « Rushes et montages… » de la page des moments). « Choisir un fichier… » ouvre l'explorateur de
   fichiers et copie la vidéo dans `montage/public/rushes/` ; « Préparer ce rush » lance la
   transcription, la découpe en moments et une première vidéo entière (environ la durée
   du rush, plus 3 à 5 min), avec l'avancement à l'écran, puis ouvre la page des moments.
   Sans consignes particulières, le premier montage garde tous les moments sauf les 1res prises.
   Consignes et style (facultatif) : écrire le montage voulu dans la zone de texte (ou choisir
   un fichier), ajouter des effets sonores (bibliothèque de sons, utilisable sur tous les
   montages) et des vidéos d'exemple dont Claude s'inspire. Si Claude Code est connecté
   (page « Connexion à Claude », une fois), il les applique pendant la préparation ; sinon, une
   bannière indique ce qui ne fonctionne pas.
2. **Avec Claude** (facultatif, ou sur claude.ai) : lui donner le rush (fichier ou lien de téléchargement)
   et les consignes de montage (`instructions.md`) ; il suit `CLAUDE.md` : suggestions de
   moments argumentées, intro, effets, coupes au mot près.
3. **Choisir** sur la page des moments : cocher les moments, corriger les sous-titres, ajouter
   des effets sonores (caisse, bop, clavier, voix modifiée) et visuels (texte tapé, pastille),
   couper un moment (frise du moment, sous celle du rush, puis « Couper ici »), zoomer d'un cadre à l'autre (« Zoom libre »),
   garder des plans sans parole, créer des variantes.
   Chaque moment se lit habillé comme dans le montage.
   Préparer un autre rush met le montage en cours de côté : la page « Rushes et montages » le rouvre
   plus tard tel quel (section « Montages », sans rien recalculer).
4. **Générer la vidéo entière** (bouton de la page), la regarder sur la page.
   En local, « Discuter avec Claude » (<http://localhost:8080/chat/>) permet aussi de lui
   demander les changements en français, sans terminal (Claude Code connecté une fois).
5. **Rendu final 4K** une fois validé : en local, « Télécharger » > « Créer la version 4K » (sur
   la page des moments ou des téléchargements ; long, l'avancement s'affiche) ; sinon le
   demander à Claude. Les versions se téléchargent depuis « Télécharger » ou la page des
   téléchargements.
## Fonctionnement
 
```
rush ──prepare──> transcription par segment (Parakeet) + visage + bruitages
     ──make_moments──> page des moments (extraits vidéo, habillage calculé dans la page)
page ──choix (sélection, corrections, effets, découpes)──> use_selection
     ──build_edit──> montage (clips, sous-titres, zooms, effets, voix : src/data/edit.json)
     ──render apercu | final──> rendu HyperFrames : out/montage_apercu.mp4 (1080x1920), out/montage_4k.mp4
```
 
- `montage/` : scripts du montage et du rendu HyperFrames, gabarits des pages ; le détail de chaque étape
  est dans [`montage/README.md`](montage/README.md).
- `CLAUDE.md` : déroulé suivi par Claude pour un nouveau rush.
- `montage/rust/` : le programme `auto-montage` (Rust), lancé par `montage/am <commande>`
  (`montage\am.cmd` sous Windows) ; `start` et `stop` lancent et arrêtent l'interface locale.
- `tests/` : test de bout en bout (rush de synthèse, installation par `auto-montage start`,
  préparation et génération par l'interface), lancé par GitHub Actions sur Linux, macOS et
  Windows à chaque modification sur main et rust (`.github/workflows/tests.yml`), ou sur sa
  machine (`tests/e2e.sh`).
- `packaging/` : installateurs (AppImage, .exe, .dmg) construits et publiés par GitHub Actions
  quand un tag de version `v*` est poussé (`.github/workflows/installateurs.yml`) ; voir
  [`packaging/README.md`](packaging/README.md).
- Non versionnés : rushes (`montage/public/rushes/`), fichiers de travail (`montage/work/`),
  rendus (`montage/out/`) et la police Oliver (usage personnel, téléchargée par
  `./am fetch_fonts`).
## Licence
 
Auto-montage est distribué sous licence [GNU GPL version 3](LICENSE). La police Oliver n'est
pas fournie (usage personnel). Les extraits et captures du tutoriel ont leur propre licence
(CC BY-SA 4.0, Pixabay) : voir les fichiers `LICENCE.txt` de `montage/local/tutoriel/`.
 
