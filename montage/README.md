# Montage automatique de rushes

Projet qui monte un rush (vidéo face caméra) selon les consignes de `instructions.md`, rendu avec
[HyperFrames](https://github.com/heygen-com/hyperframes) (page HTML + GSAP rendue par Chrome sans écran) :
dérush (blancs, ratés, doublons : 2e prise gardée), zooms alternés, voix qui tremble
sur les moments drôles (l'effet visuel sur le visage est fait à la main), bruitages, intro tapée au clavier et sous-titres.

## Voir et exporter

```bash
npm i
./scripts/render.sh apercu       # 1080x1920, ~3 s de rendu par seconde de montage : pour valider
./scripts/render.sh final        # 4K 2160x3840, une fois le montage validé
```

- **Rendu** : `scripts/render_hyperframes.py` écrit le montage (`src/data/edit.json`,
  `face.json`) en page HTML + GSAP dans `work/rendu_hyperframes/`, puis `hyperframes render`
  la filme image par image. Les effets (zooms centrés sur le visage, secousses, flashs, textes
  tapés, pastilles, sous-titres, mot prononcé en couleur, rebond) sont dessinés dans cette page ;
  la page des moments les imite en direct.
- **Aperçu** : lit la version 1080 du rush (`rush_1080.mp4`) → `out/montage_apercu.mp4`.
- **Final** : `scripts/make_hd.py` encode en 4K les seules images gardées au montage
  (`rush_2160.webm`, ~8 min pour 40 s d'images, refait seulement si le dérush change),
  puis la même page est rendue en 4K (2 navigateurs au plus, pour la mémoire) → `out/montage_4k.mp4`. Le texte reste
  net et l'image est en 4K native. Les positions de chaque clip dans la source 4K
  (`trimBeforeHd`) sont calculées par `build_edit.py`.
- Les deux modes vérifient ensuite la durée, les dimensions et la saturation du son.

## Choisir les moments (cases à cocher)

1. `./scripts/prepare.sh MonRush.mov` fait le travail lourd : parole détectée, blancs retirés,
   transcription par segment (`work/segments.json`).
2. Claude peut écrire ses suggestions dans `work/suggestions.json`
   (`{"keep": [ids], "reasons": {"id": "aparté"}}`). Sans elles, la pré-sélection est
   automatique : une phrase reprise juste après est une 1re prise, décochée.
3. `python3 scripts/make_moments.py` génère la page `work/moments/index.html` (gabarit
   `selection/page.html`), un extrait vidéo et une vignette par moment, et une copie compressée
   de la version de travail (`out/montage_apercu.mp4`) avec la correspondance clip → moment.
   Claude la publie comme page claude.ai avec le stockage `db` et les fichiers
   (`clips/*.mp4`, `thumbs/*.jpg`, `hf/*.js`, `montage.mp4` ; `hf/` : aperçu des voix faites par
   HyperFrames).
4. Sur la page : regarder la version de travail (le moment en cours est surligné), voir chaque
   prise en vidéo, cocher ou décocher, créer des variantes nommées (collection `variantes`),
   corriger le texte d'un moment et cocher ses effets sonores (collection `moments`,
   commune à toutes les variantes).
   Chaque moment se lit habillé comme dans le montage (sous-titres Oliver, zooms, étapes,
   intro, effets cochés, voix modifiée) : la page calcule cet habillage elle-même à chaque
   lecture, donc une correction ou un effet modifié se voit en relançant le moment, sans nouveau
   rendu.
5. « Générer la version de travail » (ou une demande dans le chat) réveille la session
   Claude : la page écrit la variante dans le document `demandes/regen` et programme dans
   ~1 min la routine dont l'id est dans `work/regen_trigger.txt` (connecteur
   Claude Code Remote, outil `update_trigger` avec `run_once_at`). Une exécution programmée
   arrive dans la session qui a le rush ; `fire_trigger` ouvrirait une session vide. Claude exporte la
   variante et les moments (ArtifactData avec `out_dir`), puis
   `python3 scripts/use_selection.py <export> <variante>` écrit `work/selection.json`, que
   `build_edit.py` utilise à la place de `KEEP`. Les textes corrigés sont réalignés mot à mot
   sur les horodatages d'origine. Chaque effet coché a ses réglages : départ (en s depuis le
   début de l'extrait du moment, bouton « Ici » pendant la lecture), répétitions, intervalle et
   durée ; par défaut la caisse reste sur son mot et le clavier fait 6 frappes.
   « Voix modifiée » (voix qui tremble : vibrato et trémolo) se règle par départ, durée et
   force (curseur de 0 à 200 %, 100 % = réglage d'origine, audible en direct dans l'aperçu) ;
   elle est pré-cochée sur les moments drôles (`FUNNY`). Les moments dont les effets ont été
   enregistrés avant cet effet (sans `fxv: 2`) gardent la voix proposée.
   Le curseur « Découpe » d'un moment (image et mot à l'instant choisi, « ‹ › » une image,
   « « » » » cinq images, « Zoom » sur 2 s pour placer chaque image à la souris) puis « Couper
   ici » découpe un moment en parties à cocher une à une, par exemple pour retirer une
   répétition que Whisper n'a pas transcrite. La coupe tombe sur l'image choisie (positions
   calées sur les images du rush, 30 i/s) ; dans la variante, la partie k du moment id s'écrit
   `"id/k"` et les instants de coupe sont dans le document du moment (`cuts`).
   Filtre « Sans parole » : les plans entre deux phrases (id ≥ 1000), décochés par défaut,
   à garder en entier ou en partie (« Garder de … à … », ou curseur « Bornes » avec « Début
   ici » / « Fin ici »).
   Effets visuels par moment : « Texte tapé » (comme l'intro : texte, départ, durée de frappe,
   durée d'affichage, son clavier) et « Pastille » (comme les étapes, son bop). L'intro et les
   étapes sont ces effets, pré-cochés ; pas de sous-titre pendant un texte tapé.
6. `python3 scripts/build_edit.py`, puis `./scripts/render.sh apercu` pour valider,
   et `./scripts/render.sh final` pour la 4K. Relancer `make_moments.py` et republier la page
   pour y mettre la nouvelle version de travail.

## Télécharger les versions

`python3 scripts/make_downloads.py` prépare la page « Téléchargements » (gabarit
`selection/downloads.html`) avec les versions qui existent : légère (720p, celle de la page des
moments), 1080p (`out/montage_apercu.mp4`) et 4K (`out/montage_4k.mp4`). Un fichier publié est
limité à 15 Mo : au-delà, la version est découpée en morceaux de 14 Mo que la page rassemble en
un seul MP4 au téléchargement. Une page est limitée à 256 Mo : si l'ensemble dépasse, la copie
4K de la page est réencodée (2 passes) juste assez pour tenir ; le rendu 4K d'origine reste
intact. `work/downloads/publish.json` donne les lots à publier (64 Mo max par publication) et
les morceaux de l'ancienne version à retirer. Une version plus ancienne que la version de
travail (ex. une 4K d'un montage précédent) est signalée sur la page.

## En local (Docker)

`docker/README.md` à la racine du dépôt : conteneur avec Claude Code et toutes les
dépendances, raccourci de bureau, interface web sur <http://localhost:8080/> (page des
moments et téléchargements, servis par `scripts/local_server.py`).

## Environnement cloud

Les domaines à autoriser dans l'environnement Claude Code (menu de l'environnement >
Edit > Network access) sont listés dans `env/allowed-domains.txt` :
Hugging Face et dafont. Le dépôt ne peut pas appliquer ce réglage lui-même.
`./scripts/check_network.sh` vérifie qu'ils répondent et nomme ceux qui manquent.
GitHub, npm et PyPI sont ouverts par défaut.

Le plugin Claude Code HyperFrames est déclaré dans `.claude/settings.json`.
Au démarrage de chaque session web, le hook `.claude/hooks/session-start.sh` (déclaré dans
`.claude/settings.json`) réinstalle ce plugin si besoin, puis les paquets Node et Python, le navigateur de rendu, Whisper et ses modèles, la police
Oliver et le modèle de détection du visage. Environ 2 min 30 la première fois, quelques secondes ensuite.

## Refaire la préparation

Les rushes ne sont pas dans git. Déposer le rush dans `public/rushes/`, puis lancer la préparation
avec son nom de fichier :

```bash
pip install -r scripts/requirements.txt
./scripts/prepare.sh MonRush.mov
```

Plusieurs vidéos pour un même rush (caméra coupée en cours de tournage…) : les donner dans
l'ordre, `./scripts/prepare.sh Partie1.mov Partie2.mov`. `scripts/join_rushes.py` les assemble
en « Partie1 (assemblage de 2).mov » (sans réencodage si les réglages sont identiques, sinon
réencodées aux réglages de la première), puis la préparation continue sur ce rush assemblé.

| Étape | Script | Sortie |
| --- | --- | --- |
| Version de travail 1080×1920 (MP4) | `prepare.sh` | `public/rushes/` |
| Source 4K des images gardées (rendu final) | `make_hd.py` via `render.sh final` | `public/rushes/rush_2160.webm` |
| Segments de parole (VAD Silero) et transcription mot à mot par segment (Whisper large-v3-turbo) | `setup_whisper.sh`, `transcribe_segments.py` | `work/segments.json` |
| Position du visage | `detect_face.py` | `src/data/face.json` |
| Bruitages provisoires | `make_sfx.py` | `public/sfx/` |
| Dérush, sous-titres, zooms, effets, piste voix | `build_edit.py` | `src/data/edit.json`, `public/audio/voice.wav` |

Les choix de montage sont en tête de `scripts/build_edit.py` : `KEEP` (segments gardés),
`FIXES` (corrections des sous-titres), `FUNNY` (voix qui tremble), `CASH`, `CHIPS` et `INTRO_TEXT`.
Ces listes désignent des segments de la transcription du rush en cours (`work/segments.json`) :
elles sont à refaire pour chaque nouveau rush. Relancer `python3 scripts/build_edit.py` après modification.

## À remplacer

- **Police Oliver** (Iva Florina, dafont) : gratuite pour un usage personnel uniquement,
  donc non versionnée. `scripts/fetch_fonts.sh` la télécharge dans `public/fonts/`
  (appelé par `prepare.sh`). Pour une vidéo commerciale, acheter une licence auprès de
  l'autrice. Sans le fichier, Patrick Hand sert de remplaçant.
- **Bruitages** : `public/sfx/bop.wav`, `cash.wav`, `key-1.wav` à `key-4.wav` sont synthétisés.
  Les remplacer par de vrais sons en gardant les mêmes noms.

## Notes techniques

- Navigateur de rendu (`scripts/ensure_browser.sh`) : le Chromium de l'environnement cloud
  (`/opt/pw-browsers`) s'il existe ; sur Linux ARM64 (conteneur sur un Mac Apple Silicon, où
  Google ne publie pas de Chrome sans écran), celui de Playwright dans `~/.cache/ms-playwright` ;
  ailleurs celui de HyperFrames (`hyperframes browser ensure`). Téléchargé une fois.
- HyperFrames lit les vidéos avec ffmpeg (images extraites) : la version de travail reste en
  H.264, la source 4K en VP9.
