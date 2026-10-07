# Auto-montage

Montage automatique de rushes face caméra, rendu avec HyperFrames, dans `montage/`. Tout le
détail est dans `montage/README.md` ; les consignes de montage viennent de l'utilisateur
(fichier `instructions.md` fourni dans le chat).

## Déroulé pour un nouveau rush

1. Récupérer le rush (fichier ou lien de téléchargement fourni) dans `montage/public/rushes/`.
2. `./scripts/prepare.sh <fichier>` depuis `montage/` (plusieurs vidéos pour un rush :
   `prepare.sh <vidéo 1> <vidéo 2> …`, assemblées dans l'ordre par `scripts/join_rushes.py` ;
   rush en anglais : `prepare.sh --langue en <fichier>`, français par défaut, choix gardé dans
   `work/langue.txt`). Toutes les pistes son de la vidéo sont mélangées (`moments_lib.audio_map`).
3. Lire `work/segments.json`, écrire `work/suggestions.json` (garder / retirer avec raison :
   doublon 1re prise, raté, aparté), puis `python3 scripts/make_moments.py`. Consignes de
   montage : celles du chat, ou `work/instructions.md` (choisi sur la page « Rushes et montages »).
4. Publier `work/moments/index.html` comme page claude.ai avec `capabilities: {db: {}}` et
   les fichiers `clips/*.mp4`, `thumbs/*.jpg`, `hf/*.js` et `montage.mp4`, et donner le lien à
   l'utilisateur.
   Republier au même lien (même chemin de fichier) après chaque nouvel aperçu, avec
   `capabilities: {db: {}, downloads: true, mcp: {servers: [{server: "Claude Code Remote", tools: ["update_trigger"]}]}}`.
5. Quand il a choisi (bouton « Générer la version de travail » de la page, qui écrit la
   variante dans `demandes/regen` et programme la routine dans ~1 min, ou demande dans le
   chat) : exporter `variantes/<variante>` et la
   collection `moments` avec ArtifactData (`out_dir` = `work/page_export`), puis
   `python3 scripts/use_selection.py work/page_export <variante>`. Pour un nouveau rush,
   écrire les choix de montage (coupes au mot près, corrections, voix modifiée, caisse,
   pastilles, intro) dans `work/edit_choices.json` (format en tête de `scripts/build_edit.py`,
   près de `CHOICES_PATH` ; sans ce fichier, montage générique : moments suggérés, sans
   effets ni intro), et créer la routine de régénération (create_trigger sans horaire,
   liée à la session) dont l'id va dans `work/regen_trigger.txt`. La page la programme
   (update_trigger avec `run_once_at`) : un lancement immédiat (fire_trigger) ouvre une
   session vide, sans le dépôt ni le rush, au lieu de réveiller celle-ci.
6. `python3 scripts/build_edit.py`, `./scripts/render.sh apercu` → faire valider.
7. Après validation seulement : `./scripts/render.sh final` (4K).
8. Après chaque rendu (aperçu ou 4K) : `python3 scripts/make_downloads.py`, puis publier
   `work/downloads/index.html` (page « Téléchargements », lien dans `work/downloads_url.txt`,
   `capabilities: {downloads: true}`) avec les fichiers de `work/downloads/publish.json` :
   un appel par lot de `batches` (`root` = `work/downloads`, 64 Mo max par appel), et au
   premier les chemins de `remove` à `null`. Moins de 15 Mo : un seul fichier ; au-delà,
   morceaux de 14 Mo rassemblés par la page. Si l'ensemble dépasse 245 Mo, la copie 4K de
   la page est réencodée pour tenir (256 Mo max par page).

## Versions publiées (installateurs)

Après une nouvelle fonctionnalité importante pour l'utilisateur (nouvelle page, nouveau
bouton, nouvel effet, correction d'un blocage), dans le même push : passer `VERSION` au
numéro suivant (0.2 → 0.3) et ajouter en tête de `CHANGELOG.md` une section `## 0.3` qui dit
ce qui change, en mots d'utilisateur. La version est publiée en poussant le tag `v<numéro>` sur
le commit de main qui porte ce numéro (`git tag v0.44 && git push origin v0.44`, par
l'utilisateur) : `installateurs.yml` (GitHub Actions, dépôt public) construit et vérifie les trois
installateurs, puis publie la version avec eux et les notes de `packaging/notes-version.sh
--installateurs`. Sans Actions, `packaging/publish-release.sh` publie la version sans
installateurs (la page « Mises à jour » n'a besoin que du tag ; `--essai` montre les notes) ;
un tag poussé ensuite y ajoute les installateurs. Les tests (`tests.yml`) se lancent quand une
modification du projet arrive sur main, et chaque lundi . Les installateurs ne sont construits que
pour publier une version (tag `v*`). Rien ne se lance sur dev. Pas de nouveau numéro pour une retouche mineure ou
interne : les changements attendent la prochaine version.

## À savoir

- Les rushes, fichiers de travail (`work/`) et rendus (`out/`) ne sont pas versionnés.
- La police Oliver n'est pas versionnée (licence usage personnel) : `scripts/fetch_fonts.sh`.
- L'effet visuel « visage gêné » est fait à la main par l'utilisateur ; seule la voix
  tremble automatiquement.
- Domaines réseau nécessaires : `montage/env/allowed-domains.txt`.
- Quand le rush change, `prepare.sh` met le montage de l'ancien rush de côté dans
  `work/projets/<rush>/` (`scripts/project.py save` : work/ hors fichiers communs, versions de
  travail, rendus, choix de la page) ; `project.py open <dossier>` le rouvre sans rien recalculer
  (page « Rushes et montages » /rush/, section « Montages »). Les tâches de l'interface locale
  s'annulent (`/api/job/cancel`, groupe de processus) ; les vidéos longues s'écrivent en
  `*.part.*` puis sont renommées, et une annulation efface ces fichiers provisoires.
- En local (Docker, `docker/README.md`) : pas de page claude.ai à publier ; la page
  <http://localhost:8080/rush/> envoie le rush et les consignes, et lance la préparation par
  boutons ; avec des consignes et Claude connecté, elle lance `scripts/apply_instructions.sh`
  (Claude sans interaction : `work/suggestions.json` et `work/edit_choices.json`). La page
  <http://localhost:8080/chat/> discute avec Claude (`claude -p --resume`, outils limités à la
  lecture, à `work/` et aux scripts de montage : liste `CHAT_TOOLS` de `local_server.py`).
  Éléments de style : sons personnels dans `public/sfx/perso/<nom>.wav` (non versionnés,
  communs à tous les montages ; effets « perso-<nom> » de la page, `"sfx"` d'edit_choices),
  vidéos d'exemple dans `work/style/videos/` avec une planche `*.planche.jpg` que Claude lit ;
  son analyse va dans `work/analyse_style.md` (montrée sur la page des moments). Bouton
  « Analyser les vidéos d'exemple » (tâche `style`) : `scripts/analyze_style.py` mesure (coupes,
  attaques sonores, planches datées dans `work/style/analyse/`, `faits.json`), puis
  `scripts/style_claude.sh` fait écrire à Claude `work/style/analyse/resultat.json` (éléments
  repérés, reproductibles ou non, style de sous-titres proposé), repris par la préparation.
- Sécurité de l'interface locale (`local_server.py`) : seuls les noms localhost, 127.0.0.1,
  [::1] (et `AM_ALLOWED_HOSTS`) sont servis, les écritures d'une autre origine sont refusées
  (`parse_request`). Claude sans interaction (discussion, `apply_instructions.sh`,
  `style_claude.sh`) n'écrit que dans `montage/work/`, avec les interdits de `CHAT_DENIED`
  (conversation, pages servies). Données insérées dans une page : `<` échappé.
  `src/data/edit.json` et `face.json` du dépôt sont ceux du rush de démonstration
  (`tests/make_demo_rush.py`) : n'y jamais versionner un vrai rush ; `update.py` garde ceux de
  l'utilisateur.
- Bandeau et menu des pages de l'interface locale : `local/menu.js`, ajouté par
  `local_server.py` à toutes ses pages (liste `PAGES`) ; pas de liens de navigation dans les pages.
  Il marque aussi les parties qui ont besoin de Claude Code (attribut `data-needs-claude`) :
  pastille « Nécessite Claude Code », grisées et `inert` sans connexion (`/api/status`).
- Sous-titres : jusqu'à 6 mots sur 2 lignes par défaut, coupées là où les largeurs mesurées (multipliées
  par la taille de chaque ligne) sont les plus proches (`render_lib.caption_layout` et `captionLayout` de la
  page des moments) ; nombre de lignes au plus (`lines`, 1 à 3, mots par sous-titre :
  `fonts_lib.CAPTION_LIMITS`) et taille de chaque ligne en % de la première (`lineSizes`), réglés sur la
  page des moments et sur /rush/ ; sous-titre remonté s'il dépasse 40 px du bas. Style (police, graisse,
  taille, majuscules, couleur, lignes) : `work/caption_style.json` (`scripts/fonts_lib.py` : polices
  d'Auto-montage, de la machine de rendu via fontconfig, et `public/fonts/perso/`), copié par
  `build_edit.py` dans edit.json (`captionStyle`, police dans `public/fonts/choisie/`).
  Effets proposés sur la page des moments : `work/effets_page.json` (`hidden`, et `volume` de
  tous les effets sonores), réglé sur /rush/ et la page des moments ; volume par son et par
  moment : `vol` de chaque effet (1 = 100 %), appliqué par `build_edit.py`.
- Son de la voix (page des moments, panneau « Son de la voix ») : `work/son.json` (volume
  `voice`, `denoise` 0-3, `declick`, `highpass`, `clarity`, `mute` = fenêtres en temps du rush
  atténuées de 15 dB) et `gain` par moment, appliqués par `build_edit.py` (filtres ffmpeg sur la
  piste voix), sauf `highpass` et `clarity`, faits au rendu par HyperFrames (`selection/hf_audio.js`,
  avec le limiteur à -1,5 dB) ; `scripts/analyze_sound.py` (bruit de fond, clics et chocs) → `work/son_analyse.json`.
- Effets fournis par défaut : liste unique dans `scripts/moments_lib.py` (`SOUNDS` : sons de
  `public/sfx/`, synthétisés par `make_sfx.py` ; `VISUALS` : texte tapé, pastille, zoom avant,
  zoom sec, dézoom, secousse, flash, mot mis en valeur). `build_edit.py` en tire `zoomFx`,
  `shakes`, `flashes`, `highlights` (edit.json, rendus par `render_hyperframes.py` et l'aperçu de la page) ;
  habillage de tout le montage dans `work/habillage.json` (`autoZoom`, `pop`, `karaoke`).
  Tous les effets par défaut (sons, « voix », effets visuels) et « gainvoix » (curseur de
  volume de la voix par moment) sont écartés (`hidden`) tant que l'utilisateur ne les coche pas
  (`moments_lib.hidden_effects`, `DEFAULT_HIDDEN` par version, marque `"defauts"` dans
  effets_page.json). Voix
  modifiées (effet « voix », champ `voice`) : `moments_lib.VOICES` (tremblement, robot,
  écureuil, lutin, géant, mégaphone, téléphone, grande salle, batterie faible, vieille radio,
  chœur). Celles de `moments_lib.HF_VOICES` (mégaphone, téléphone, grande salle, écureuil,
  lutin, géant, chœur) sont faites par HyperFrames : chaînes d'effets `data-fx-chain` de
  `selection/hf_audio.js` (`voiceElements` : voix entière baissée pendant les fenêtres, un
  élément audio par fenêtre), le même code pour le rendu (`render_hyperframes.py`, par node) et
  pour l'aperçu de la page des moments, qui joue alors le son du moment dans un lecteur
  HyperFrames caché (`hfUpdate`, fichiers `work/moments/hf/` copiés par
  `moments_lib.hf_preview_files`). Les autres (tremblement, robot, vieille radio, batterie
  faible : modulation en anneau, souffle, ralentissement) restent des filtres ffmpeg
  `voice_filter(voice, force)` appliqués fenêtre par fenêtre par `build_edit.py`, imités en Web
  Audio dans l'aperçu (`buildVoiceChain`, aussi utilisé sans lecteur HyperFrames) ;
  exemples sur /rush/ : `local/tutoriel/voix/<voix>.mp3` (`tests/make_voice_demo.py`, voix Piper,
  filtres ffmpeg pour toutes les voix).
- Calage des effets sur la page des moments : `fxBar` (bande déplaçable, poignées début/fin,
  clavier image par image). Sons : durée d'une répétition = `len` du catalogue
  (`moments_lib.sound_catalog`, `wav_len`), répétitions = `repeat`, `gap` = durée + `pause` ;
  voix modifiée et effets visuels : `at` et `dur`.
- Moments scindés ou fusionnés (page des moments, interface locale : « Scinder ici »,
  « Fusionner ↓ », « Séparer », « Recoller ↑ ») : `work/decoupage.json` (`splits` : segment ->
  mots de coupe ; `merges` : ids consécutifs), `scripts/decoupage.py` (report des réglages de
  `local_db` : effets décalés, moments gardés), puis `make_moments.py` (extraits refaits
  seulement si leurs bornes changent : `clips/.bornes.json`). `segments.json` ne change pas :
  `moments_lib.load_segments` / `effective_segments` donnent les moments effectifs (partie
  scindée : `part_id(segment, mot)` ≥ `SPLIT_BASE` ; fusion : id du premier) ; `build_edit.py`
  convertit les choix d'edit_choices (ids et mots d'origine) avec `word_map`. Une fusion garde
  la pause entre les moments (extrait continu).
- Sous-titres supprimés pour un moment (page des moments, « Corriger » : texte vide ou
  « Supprimer les sous-titres ») : `"nosub": true` dans le document du moment, transmis par
  `use_selection.py` comme correction vide, que `build_edit.py` applique (mots sans texte).
- Rendu : `scripts/render.sh apercu|final` lance `scripts/render_hyperframes.py`, qui écrit le
  montage (edit.json, face.json) en page HTML + GSAP dans `work/rendu_hyperframes/` et la rend
  avec `hyperframes render` (Chrome sans écran : `render_lib.headless_chrome` → `scripts/ensure_browser.sh`, Chromium de
  Playwright sur Linux ARM64 (Mac Apple Silicon), sinon `hyperframes browser ensure` ; ffprobe de `@ffprobe-installer`). Les effets sont dessinés image
  par image dans cette page (fonction `draw`) ; la page des moments les imite en direct : un effet
  ajouté doit l'être aux deux. Python (`scripts/render_lib.py`) : bruitages, police des
  sous-titres (Pillow), mots mis en valeur, coupure en 2 lignes. 4K : 2 navigateurs au plus
  (4 dépassent 14 Go de mémoire). Whisper : `scripts/setup_whisper.sh` (whisper.cpp 1.7.6
  compilé, modèles de Hugging Face).
- Tutoriel (/tutoriel/, `local/tutorial.html`) : ouvert une fois au premier lancement (menu.js,
  `work/tutoriel_vu`) ; trois onglets (sans Claude Code, avec, « Fonctionnalités en détail » :
  une fiche par sujet, sons et voix à écouter) ; captures et vidéos dans `local/tutoriel/`,
  tirées d'une intervention en 16:9 sous CC BY-SA (Wikimania 2023, Wikimedia Commons : crédit en
  bas de la page et `local/tutoriel/LICENCE.txt`, à garder) montée dans l'interface avec les
  consignes citées dans l'onglet Claude. À refaire quand l'interface change beaucoup.
- Format : celui du rush. `prepare.sh` fait la version de travail aux proportions du rush, petit
  côté à 1080 (`moments_lib.work_size` : 1080x1920, 1920x1080, 1440x1080 en 4:3… ; `setsar=1` ;
  refaite si sa taille ne correspond plus) ; `build_edit.py` écrit `width`/`height` dans edit.json
  (`moments_lib.frame_size`), repris par le rendu (positions de l'habillage :
  `moments_lib.frame_layout`, sous-titres plus bas sur une image en largeur ; 4K par
  `--resolution` pour 9:16, 16:9 et carré, sinon agrandie ×2 dans la page, `HD_PRESETS`),
  `make_hd.py`, la page des moments (`--ar`, `--u` = 10,8 px de la composition, classe `paysage`
  pour le lecteur large) et les téléchargements (`res_label`).
- Mémoire : `scripts/memoire.py` (besoin de chaque opération d'après la taille de l'image et la durée,
  `OPERATIONS` / `STEPS` ; mémoire libre bornée par la limite du conteneur) ; avertissement « juste » ou
  « insuffisant » dans prepare.sh, sur /rush/ avant de préparer (`/api/memoire`), au début de chaque
  étape (`check_memory`) et si la mémoire s'épuise (`watch_memory`), 4K comprise (`final_info`).
- Pause et reprise de la préparation (/rush/ : « Mettre en pause », « Reprendre », « Abandonner ») :
  `work/tache.json` (étapes, étape atteinte, avancement ; `save_task`, `paused_task`, `resume_job` de
  `local_server.py`), resté « running » après un arrêt = coupée, reprenable aussi après une erreur ;
  reprise = étape relancée avec `AM_REPRISE=1` (`prepare.sh --reprendre`), qui garde les morceaux de
  `version_travail.py` (`work/reprise/travail/`), les morceaux transcrits (`work/seg/cle.txt`), les
  visages (`work/reprise/visage.json`) et les extraits de la page. Tant qu'elle attend, les autres
  tâches sont refusées (`busy_error`), sauf mise à jour ; tache.json et reprise/ ne sont jamais rangés
  par `project.py`.
- Barres de progression de l'interface locale : un script long écrit des lignes
  `@progression <fait> <total> <libellé>` (`scripts/progress.py`, aussi pour suivre un ffmpeg) ;
  `local_server.py` les lit. Fait et total en secondes de vidéo (rush, ou montage pour le rendu :
  `UNITS`), montrés par opération sous chaque étape de /rush/ (`job.ops`). Temps restant : `PLAN` et `RATES`
  (secondes par seconde de rush ou de montage), recalés sur la machine dans `work/timings.json`.
- Connexion à Claude (/claude/) : `claude auth login` dans un pseudo-terminal (adresse
  d'autorisation renvoyée à la page, code collé transmis) ; état par `claude auth status`.
- Mises à jour (/updates/) : `scripts/update.py` (check : versions publiées via l'API GitHub,
  sans jeton, dépôt public ; apply : archive du tag copiée sur l'installation, données de
  l'utilisateur gardées (`KEEP`), fichiers retirés d'après `.auto-montage-files` ; reset, bouton
  « Réinitialiser Auto-montage » après confirmation : archive de la version installée, sinon la
  dernière publiée, fichiers du programme modifiés ou effacés remis, copie des remplacés dans
  `work/sauvegardes/reinitialisation-<date>/`), puis le serveur s'arrête et le conteneur redémarre. Fichiers changés
  sur le disque sans mise à jour (git pull dans le dossier d'installation) : la page le dit et
  propose « Redémarrer » (`/api/restart`, `disk_version()`). Les installateurs
  ne recopient leur projet que s'il est plus récent (`version_gt`).
- Libérer de la place (section en bas de /rush/) : `scripts/place.py` (inventaire : montages
  enregistrés, rushes de `public/rushes`, `rush_2160.webm`, vidéos de `work/rendu_hyperframes/assets`,
  `work/sauvegardes`, restes `.envoi-*` et `*.part.*` ; taille réellement libérée, liens physiques
  exclus), `/api/place` et `/api/place/delete` (refusé pendant une tâche). Au démarrage du serveur,
  les restes sont effacés ; `project.py save` efface les vidéos du dernier rendu. Docker : `LABEL
  auto-montage` (Dockerfile), ancienne image effacée par les lanceurs après `up --build` ; menu
  « Nettoyer Docker » (`docker/clean.sh`, `docker/windows/clean.ps1` : images sans nom, cache de
  construction, jamais les volumes).
- Journal (/logs/) : chaque tâche écrit `work/logs/<date>_<tâche>.log` (40 gardés, fin
  « # RÉSULTAT : … ») ; démarrage et serveur dans `work/logs/demarrage.log` (entrypoint) ;
  erreurs de la discussion dans `work/logs/discussion.log` ; `/api/logs/rapport` les regroupe.
  Points d'avancement avec disque libre et mémoire disponible (`progress.resources`) ; échec de
  ffmpeg (`progress.ffmpeg`) : code ou signal expliqué (`exit_reason`) et ses 40 dernières lignes ;
  au démarrage du serveur, un journal sans « # RÉSULTAT » est fermé en « interrompu »
  (`close_interrupted_logs`) ; journal impossible à écrire : signalé sur la page et dans demarrage.log.
- Tests : `tests/e2e.sh` (rush de synthèse de `tests/make_sample_rush.sh`) dans le workflow
  `tests.yml` (quand le projet change sur main, chaque lundi, ou à la main). L'interface
  <http://localhost:8080/> sert `work/moments/` telle quelle (relancer `make_moments.py`
  suffit), les choix sont dans `work/local_db/` (même forme que l'export ArtifactData :
  `python3 scripts/use_selection.py work/local_db <variante>`) et son bouton « Générer »
  monte la version de travail lui-même (`scripts/local_server.py`). « Télécharger » y propose
  720p, 1080p et la création de la 4K (`/api/final` : `make_hd.py` puis `render.sh final`).
