# Nouveautés

Chaque version publiée reprend sa section dans ses notes (onglet Releases). Une nouvelle
section avec un nouveau numéro dans `VERSION` suffit à publier la version (voir
`packaging/README.md`).

## 0.43

- **Les vidéos gardent leur format (16:9, 4:3…)** : avant, un rush filmé en paysage ou en 4:3
  était étiré en portrait. Maintenant, la version de travail, la page des moments, la vidéo
  finale et les téléchargements gardent le format du rush : 1920x1080 en 16:9 (3840x2160 en
  4K), 1440x1080 en 4:3 (2880x2160 en 4K), et de même pour les autres proportions. Sur une
  image en largeur, les sous-titres sont placés un peu plus bas. Un rush déjà préparé est refait
  au bon format à sa prochaine préparation. Rien ne change pour un rush en portrait.
- **Tutoriel refait sur une vraie vidéo**, une intervention filmée de Wikimania 2023 (licence
  CC BY-SA, crédit en bas de la page), montée en une minute avec les consignes données en exemple.
- **Nouvel onglet « Fonctionnalités en détail »** dans le tutoriel : découpe, effets fournis
  (sons et voix modifiées à écouter), sous-titres, son et lecture, sujet par sujet.

## 0.42

- **Ombre des sous-titres réglable en trois gestes** (panneau « Style des sous-titres » de la
  page des moments) : l'**intensité** rend l'ombre plus ou moins sombre, la **diffusion du
  flou** la rend plus nette ou plus étalée, et un **cercle** choisit d'un clic à la fois la
  direction et la distance de l'ombre (le centre la laisse autour du texte, le bord l'éloigne
  le plus). Les ombres réglées avant gardent leur direction.

## 0.41.4

- **Rendu de nouveau possible sur Mac (Apple Silicon)** : depuis la 0.40, l'installation sur
  Mac se plaignait d'un Chromium absent et ne pouvait rien rendre. Google ne fournit pas le
  navigateur de rendu pour ces Mac (Linux ARM dans le conteneur) : Auto-montage télécharge
  maintenant celui de Playwright, une fois, au démarrage.

## 0.41.3

- **Aperçu des effets en vidéo sur la page « Rushes et montages »** : les effets visuels se
  montrent maintenant sur une courte vidéo libre de droits (Pixabay, fond vert remplacé par une
  pièce claire et floue) au lieu d'une image fixe : une phrase prononcée, de la durée de
  l'exemple. Elle n'apparaît
  toujours que pour les effets visuels, pas pour les sons ni les voix.

## 0.41.2

- **Aperçu des effets sur la page « Rushes et montages »** : les effets visuels se montrent sur
  l'image d'une vidéo libre de droits (Pixabay) au lieu d'une image de vos vidéos. Cette
  vignette n'apparaît que quand on essaie un effet visuel ; elle reste cachée pour les sons et
  les voix. Les sous-titres de l'aperçu sont aussi plus lisibles.

## 0.41

- **Voix modifiées faites par HyperFrames, entendues pareil dans l'aperçu** : mégaphone,
  téléphone, grande salle, écureuil, lutin, géant et chœur. Sur la page des moments, le son d'un
  moment qui en a passe par le lecteur HyperFrames, avec exactement les mêmes effets que la
  version de travail. Ces voix gardent leur caractère et sont maintenant au niveau de la voix
  normale (le géant, le lutin et le mégaphone étaient bien plus faibles).
- **Tremblement, robot, vieille radio et batterie faible** ne changent pas (HyperFrames n'a pas
  les effets qu'il leur faut).
- **« Couper les graves parasites » et « Voix plus claire »** (panneau « Son de la voix ») sont
  faits par HyperFrames, et s'entendent dans l'aperçu des moments qui ont une de ces voix.

## 0.40

- **HyperFrames fabrique toutes les vidéos** : la version de travail, la 1080p et la 4K. Remotion
  est retiré du projet, ainsi que le moteur d'essai ffmpeg + ASS. Le rendu garde tous les
  effets : zooms, secousses, flashs, textes tapés, pastilles, sous-titres avec mots mis en
  valeur, mot prononcé en couleur et rebond, bruitages et voix.
- **Le dossier `remotion/` s'appelle maintenant `montage/`** : rushes, montages, rendus, sons et
  polices personnels y sont déplacés tout seuls au premier démarrage. Les rushes se déposent donc
  dans `montage/public/rushes/` (le raccourci « Dossier des rushes » y mène). Après la mise à
  jour depuis l'interface, rouvrir une fois Auto-montage par son raccourci finit le passage.
- **Panneau « Moteur de rendu (essai) » et boutons « Comparer les moteurs » retirés** de la page
  des moments : il n'y a plus qu'un moteur.
- **Préparation d'un rush plus courte** : la copie WebM du rush, qui ne servait qu'à Remotion,
  n'est plus fabriquée.
- **Mise à jour plus légère** : environ 370 paquets npm en moins (Remotion, React, Tailwind).
  Le plugin Claude Code Remotion est désinstallé au redémarrage ; seul le plugin HyperFrames
  reste.
- **4K** : 2 navigateurs à la fois au plus, pour ne pas manquer de mémoire (environ 4 min de rendu
  pour 18 s de montage sur 4 cœurs).

## 0.39.4

- **« Comparer les moteurs » corrigé sur un moment sans sous-titres** (sous-titres supprimés) :
  la comparaison échouait dès le montage du moment. La même erreur pouvait bloquer « Générer la
  version de travail » si aucun moment gardé n'avait de sous-titres.

## 0.39.3

- **Comparer les moteurs sur un seul moment** : bouton « Comparer les moteurs » sur chaque moment
  de la page des moments. Le moment est rendu seul, avec ses effets et ses réglages, par Remotion,
  ffmpeg + ASS et HyperFrames. L'aperçu de la page est aussi enregistré comme 4e version, tel
  qu'on le voit et l'entend en jouant le moment (voix modifiée et bruitages compris). Les 4
  s'affichent côte à côte dans une fenêtre, et chacune s'ouvre seule avec son propre son.
  Quelques dizaines de secondes, sans toucher à la version de travail.
- **« Rendre avec les 3 moteurs » corrigé** : il monte maintenant la variante affichée avant de
  rendre (comme « Générer la version de travail »). Avant, il pouvait rendre le montage de
  démonstration et dérégler la page des moments.

## 0.39.2

- **Essai de trois moteurs de rendu** (page des moments, panneau « Moteur de rendu (essai) ») :
  le même montage peut être fabriqué par
  - **Remotion** (celui d'origine) ;
  - **ffmpeg + ASS** : sans navigateur, environ deux fois plus rapide ;
  - **HyperFrames** : navigateur Chrome comme Remotion, sous licence libre.
  Tous reprennent les mêmes effets : zooms, secousses, flashs, textes tapés, pastilles,
  sous-titres avec mots mis en valeur, mot prononcé en couleur et rebond. Le moteur choisi sert à
  « Générer la version de travail » et à la 4K.
- **« Rendre avec les 3 moteurs »** monte la variante affichée (comme « Générer la version de
  travail »), la rend avec chacun, puis montre une
  vidéo côte à côte avec la durée de chaque rendu. De quoi comparer sur vos propres montages
  avant de choisir celui à garder.

## 0.39

- **Scinder ou fusionner des moments** (page des moments) :
  - « Scinder ici », à côté de « Couper ici » : fait deux moments séparés à partir du mot le
    plus proche du curseur, chacun avec ses effets, son texte et sa case ;
  - « Fusionner ↓ » : réunit un moment et le suivant en un seul, sans coupe entre les deux (la
    pause entre les phrases est gardée) ;
  - « Séparer » et « Recoller ↑ » défont une fusion ou une scission, et chaque moment retrouve
    ses réglages.
  Les effets déjà placés suivent le moment où ils tombent. La page ne refait que les extraits
  vidéo touchés : quelques secondes seulement.

## 0.38

- **Page des moments plus légère** : tous les effets (sons, voix modifiée, effets visuels) sont
  maintenant décochés par défaut dans « Effets proposés » ; un moment n'affiche une ligne
  d'effets que si un effet y est proposé ou déjà coché.
- **Volume de la voix par moment** : son curseur n'apparaît plus par défaut. Cochez « Volume de
  la voix par moment » dans les effets proposés (groupe « Voix ») pour l'afficher ; un moment
  dont le volume a déjà été changé le garde.
- **Calage des effets à la souris** : chaque effet d'un moment (sons, voix modifiée, effets
  visuels) a une bande sur la durée du moment, comme pour la découpe. Glissez-la pour la
  déplacer, tirez ses poignées pour changer le début ou la fin (la vidéo suit). Un son dure par
  défaut sa propre durée ; allongé, il se répète, avec une pause réglable entre les répétitions.

## 0.37

- **11 voix modifiées** au lieu d'une : en plus du tremblement (plus marqué, avec un peu de
  robot), robot, écureuil, lutin, géant, mégaphone, téléphone, grande salle, batterie faible,
  vieille radio et chœur. Sur chaque moment, choisissez la voix à côté de la force ; l'aperçu
  la fait entendre tout de suite.
- Page « Rushes et montages » : le ▶ de « Voix modifiée » fait écouter chaque voix sur la même
  phrase (voix normale, puis modifiée), avec une voix de synthèse plus naturelle.

## 0.36

- **Sons par défaut décochés** : les sons fournis avec Auto-montage (caisse, whoosh, ding…) ne
  sont plus proposés sur la page des moments tant que vous ne les cochez pas (page « Rushes et
  montages », « Effets proposés »). Vos propres sons restent proposés.
- **Voix modifiée à écouter** : sur la page « Rushes et montages », le bouton ▶ de « Voix
  modifiée » montre une petite vidéo où la même phrase est dite avec la voix normale, puis
  modifiée.
- **Supprimer les sous-titres d'un moment** : dans « Corriger », enregistrer un texte vide ou
  cliquer sur « Supprimer les sous-titres » retire les sous-titres de ce moment ; « Texte
  d'origine » les remet.

## 0.35

- **Sécurité** : un site web ouvert dans le navigateur pendant qu'Auto-montage tourne ne peut
  plus lire ni piloter l'interface (adresse et origine des requêtes vérifiées). Claude, quand
  il travaille seul (consignes, analyse de style, discussion), ne peut plus modifier que les
  fichiers du montage, ni lire le jeton GitHub. La discussion n'affiche plus de lien piégé.
- Une mise à jour garde le montage en cours au lieu de le remplacer par l'exemple fourni.

## 0.34

- **Tutoriel** : nouvelle page (menu > Tutoriel), ouverte automatiquement au premier
  lancement. Deux onglets, « Sans Claude Code » et « Avec Claude Code », expliquent pas à pas
  comment utiliser Auto-montage et ce qu'il sait faire, avec des captures d'écran et de petites
  vidéos de démonstration (faites avec un personnage dessiné et une voix de synthèse).

## 0.33

- **Écouter et voir les effets** : sur « Rushes et montages », chaque effet de la liste a un
  bouton ▶. Pour un son, il le joue ; pour un effet visuel (zooms, secousse, flash, mot mis en
  valeur, pastille, texte tapé), il l'anime sur une image de votre montage en cours, avec une
  courte explication.

## 0.32

- **Nouveaux effets visuels** à placer sur chaque moment : zoom avant (progressif), zoom sec,
  dézoom, secousse, flash blanc (départ, durée et force réglables) et mot mis en valeur (un
  mot du sous-titre plus gros et en couleur).
- **Zooms et animations** (page des moments) : intensité des zooms automatiques sur le visage
  (aucun, légers, normaux, marqués), sous-titres qui apparaissent avec un petit rebond, et
  mot prononcé en couleur au fil de la parole.
- **Nouveaux sons** : whoosh, pop, ding, boum, montée, glitch, déclencheur photo, faux
  (buzzer) et juste (carillon).
- Ce sont les effets fournis par défaut ; pour un montage plus personnel, ajoutez vos propres
  sons et faites analyser des vidéos d'exemple par Claude. Claude peut aussi placer ces effets
  d'après vos consignes.

## 0.31

- **Volume de la voix** : sur la page des moments, panneau « Son de la voix » avec le volume
  de la voix de tout le montage, et un curseur « Voix » sur chaque moment (de 0 à 200 %).
- **Nettoyage du son** : dans le même panneau, réduction du bruit de fond (légère, moyenne ou
  forte), retrait des clics et claquements de bouche, coupure des graves parasites
  (ronflement, chocs sourds) et voix plus claire et plus régulière.
- **Bruits parasites** : « Rechercher les bruits parasites » mesure le bruit de fond (avec la
  réduction conseillée) et liste les bruits brefs repérés dans la version de travail (clics,
  chocs, coups sur le micro), avec « Écouter » et « Atténuer » pour chacun.
- Ces réglages s'appliquent à la prochaine « Générer la version de travail ».

## 0.30

- **Volume des effets sonores** : sur la page des moments, un curseur « Volume de tous les
  effets sonores » (de 0 à 200 %) dans « Effets proposés sur les moments », et un curseur
  « volume » sur chaque son placé sur un moment. Les deux se combinent ; l'aperçu change tout
  de suite, la vidéo à la prochaine génération.
- **Notes de mise à jour lisibles** : sur la page « Mises à jour », les nouveautés s'affichent
  en paragraphes et listes (plus de lignes coupées à 2 ou 3 mots), et celles de toutes les
  versions que vous n'avez pas encore installées sont montrées.
- L'analyse des vidéos d'exemple indique clairement qu'elle nécessite Claude Code.

## 0.29

- **Analyse des vidéos d'exemple** : sur « Rushes et montages », « Analyser les vidéos
  d'exemple » (durée prévue affichée, barre d'avancement). Auto-montage mesure d'abord le
  rythme (changements de plan, durée des plans), repère les bruitages probables et extrait des
  images datées ; puis Claude les regarde et liste ce qu'il a repéré (sous-titres, textes,
  zooms, transitions, effets, sons), chaque élément marqué « Reproductible », « En partie » ou
  « Pas reproductible » avec le réglage à utiliser. Il propose un style de sous-titres
  applicable en un clic et des conseils. La préparation suivante s'appuie sur cette analyse.
- **Ajouter des sons depuis la page des moments** : bouton « Ajouter des sons… » dans
  « Effets proposés sur les moments » ; ils apparaissent tout de suite dans les effets de
  chaque moment.

## 0.28

- **Ombre dirigée** : quand une direction est choisie, le flou de l'ombre ne s'étale plus que
  dans cette direction, en traînée qui s'estompe (plus de halo de l'autre côté du texte). Le
  halo tout autour reste disponible avec « Autour », qui devient la direction par défaut
  (proche de l'ombre d'origine).

## 0.27

- **Direction de l'ombre des sous-titres** : dans « Style des sous-titres », l'ombre peut
  partir vers le haut, le bas, la gauche, la droite, une diagonale (en haut à droite, en bas à
  gauche…), ou tout autour sans décalage.

## 0.26

- **Ce qui a besoin de Claude Code est signalé** : une pastille « Nécessite Claude Code » en
  haut à droite des parties concernées (consignes et vidéos d'exemple sur « Rushes et
  montages », zone de message de « Discuter avec Claude ») et sur « Discuter avec Claude » dans
  le menu. Quand Claude Code n'est pas connecté, ces parties sont grisées et inutilisables, et
  la pastille mène à la page de connexion.

## 0.25

- **Ombre des sous-titres réglable** : dans « Style des sous-titres », deux curseurs règlent
  l'intensité de l'ombre (de aucune à très marquée) et son flou (nette ou diffuse). Visible
  tout de suite dans l'exemple et l'aperçu des moments, dans la vidéo à la prochaine
  génération.
- **Retirer un effet partout** : quand vous écartez un effet de la liste des effets proposés,
  Auto-montage propose de le décocher aussi sur tous les moments où il est placé (tout de
  suite sur la page des moments ; depuis « Rushes et montages », à la prochaine ouverture de
  la page des moments).

## 0.24

- **Choix des effets aussi sur la page des moments** : « Effets proposés sur les moments »,
  sous le style des sous-titres, pour cocher les effets à montrer sans quitter la page (même
  réglage que sur « Rushes et montages »).
- **Barre de lecture en plein écran** : lecture / pause, curseur avec le temps écoulé et la
  durée totale, et « Quitter le plein écran ». Elle disparaît quand la souris ne bouge plus et
  revient dès qu'elle bouge.

## 0.23

- **Sous-titres sur 2 lignes équilibrées** : les deux lignes ont la même taille, et la coupure
  est choisie pour que leurs largeurs soient les plus proches possible (mesurées dans la police
  utilisée). Une ligne trop longue est réduite pour rester dans l'image.
- **Style des sous-titres** : sur la page des moments, « Style des sous-titres » permet de
  choisir la police (celles d'Auto-montage, celles installées sur la machine qui fait les
  vidéos, ou les vôtres avec « Ajouter une police… »), la graisse (normale, grasse…), la
  taille, les majuscules et la couleur, avec un exemple en direct. L'aperçu des moments change
  tout de suite ; la vidéo à la prochaine « Générer la version de travail ». Claude peut aussi
  le régler d'après vos consignes.
- **Choix des effets proposés** : sur la page « Rushes et montages », cochez les effets (sons,
  voix modifiée, effets visuels, vos sons) à proposer sur la page des moments ; les autres
  n'encombrent plus la liste de chaque moment.
- **Plein écran** : bouton « Plein écran » au-dessus du lecteur (version de travail ou moment,
  avec ses sous-titres). Clic ou Espace : lecture / pause ; Échap pour sortir.

## 0.22

- **Menu en haut de chaque page** : un bandeau « Auto-montage » avec un bouton « Menu » qui
  déroule toutes les pages (moments, rushes et montages, téléchargements, discussion avec
  Claude, connexion à Claude, mises à jour, journal). Les liens éparpillés dans les pages ont
  disparu. Une pastille sur le menu signale une nouvelle version.

## 0.21

- **Plusieurs vidéos pour un même rush** : si la caméra s'est coupée en cours de tournage
  (batterie, carte pleine, appel…), choisissez les vidéos l'une après l'autre (ou plusieurs
  d'un coup) sur la page « Rushes et montages », remettez-les dans l'ordre avec ↑ ↓, et la
  préparation les assemble en un seul rush. Vidéos aux mêmes réglages : assemblées en quelques
  secondes, sans perte. Réglages différents (autre caméra, autre taille) : réencodées aux
  réglages de la première, ce qui prend plus de temps.
- La barre de progression de la préparation n'en montre plus qu'une : l'avancement de
  l'ensemble, avec l'opération en cours écrite à côté.

## 0.20

- **Mises à jour automatiques** : Auto-montage vérifie lui-même s'il existe une nouvelle
  version (au démarrage, puis toutes les 6 heures). Un bandeau le signale ; la page « Mises à
  jour » montre les nouveautés et installe la nouvelle version en un clic, puis Auto-montage
  redémarre tout seul. Vos montages, rushes, rendus, sons, consignes et réglages sont gardés.
  Plus besoin de réinstaller, ni de réautoriser l'application sur Mac.
- Tant que le dépôt GitHub est privé : un jeton GitHub en lecture seule, à créer une fois
  (pas à pas sur la page « Mises à jour »).
- Les installateurs (Mac, Linux) ne remplacent plus une version plus récente installée depuis
  l'interface.
- Cette version-ci s'installe encore à la main (c'est elle qui apporte les mises à jour
  automatiques) ; les suivantes s'installeront depuis l'interface.

## 0.19

- **Connexion à Claude depuis l'interface** : nouvelle page « Connexion à Claude » (liens dans
  tous les avertissements « Claude Code n'est pas connecté »). « Ouvrir la page
  d'autorisation » ouvre claude.ai dans un nouvel onglet de votre navigateur : si vous y êtes
  déjà connecté, cliquez simplement sur « Autoriser », puis collez le code affiché. Plus
  besoin du terminal. La page montre aussi le compte connecté et permet de se déconnecter.
- L'état « connecté » est vérifié auprès de Claude Code lui-même (plus fiable).

## 0.18

- **Journal** pour comprendre ce qui a cloché : nouvelle page « Journal » (lien sur toutes les
  pages, et dans les messages d'échec). Elle garde le détail de chaque préparation,
  génération et création 4K (étapes, heures, messages, cause de l'échec), ainsi que le
  démarrage d'Auto-montage et les erreurs de la discussion avec Claude.
  « Télécharger le rapport » produit un fichier texte à joindre à une demande d'aide
  (version, système, état de Claude Code, dernières tâches).

## 0.17

- **Consignes tapées directement** : sur la page « Rushes et montages », une zone de texte
  pour écrire les consignes de montage, en plus du bouton pour choisir un fichier. Elles
  sont enregistrées avant la préparation.
- **Éléments de style** :
  - **Effets sonores** : vos sons (.wav, .mp3, .m4a…) rejoignent une bibliothèque commune à
    tous les montages. Ils apparaissent dans les effets de chaque moment (à cocher, à placer
    au curseur, à répéter) et Claude peut les placer pendant la préparation.
  - **Vidéos d'exemple** : Claude en regarde 12 images pour s'inspirer du rythme, des textes à
    l'écran et des effets. Il reproduit ce que le montage sait faire et note ce qu'il n'a pas
    pu reproduire (police, effet visuel particulier) dans une « Analyse du style », affichée
    en haut de la page des moments.
- **Sans Claude Code** : un avertissement sur les pages indique ce qui ne fonctionne pas sans
  Claude Code connecté (consignes et style appliqués à la préparation, discussion) et ce qui
  marche quand même (tout le reste, y compris vos sons).

## 0.16

- **Mac : première ouverture expliquée** : macOS bloque Auto-montage (« Apple n'a pas pu
  vérifier… ») car l'application n'est pas notarisée par Apple. Le `.dmg` contient maintenant
  un fichier « LISEZ-MOI - première ouverture » avec deux façons de l'autoriser, une fois par
  version : le bouton « Ouvrir quand même » des Réglages Système, ou une ligne à coller dans
  le Terminal. Les notes de version et le README donnent les mêmes étapes.

## 0.15

- **Découpe à l'image près** : la coupe tombe exactement sur l'image choisie au curseur
  (avant, le montage pouvait la déplacer jusqu'à 0,12 s pour la caler sur un silence). Le
  curseur avance image par image (1/30 s), « « » et « » » de 5 images, et **« Zoom »**
  l'étale sur 2 s autour de la position pour placer chaque image à la souris (« Vue entière »
  pour revenir). Aussi pour les bornes des plans sans parole.
- Les coupes déjà faites peuvent bouger d'une ou deux images à la prochaine génération (elles
  ne sont plus recalées) : vérifiez-les avec le curseur si besoin.

## 0.14

- **Curseurs plus précis** : le curseur de découpe est maintenant une barre verticale (comme
  les repères de coupe), plus facile à placer qu'un rond.
- **Curseur pour les plans sans parole** : la ligne « Bornes » montre la partie gardée en
  couleur ; en faisant glisser le curseur, le lecteur affiche l'image, ‹ › avancent d'une
  image, et « Début ici » / « Fin ici » placent les bornes à l'instant choisi.

## 0.13

- **Bouton « Annuler »** pour les opérations longues : préparation d'un rush, génération de la
  version de travail et création de la 4K. L'opération s'arrête tout de suite ; la version
  précédente reste intacte (aucun fichier à moitié écrit n'est gardé).
- **Reprendre un montage, plus visible** : la page « Nouveau rush » devient « Rushes et
  montages ». La section « Montages » montre toujours le montage en cours (« Ouvrir ») et ceux
  mis de côté (« Rouvrir »). En choisissant un rush qui a déjà un montage, « Rouvrir ce
  montage » apparaît à côté du bouton de préparation, renommé « Tout refaire depuis le début ».
- Estimation du temps de création de la 4K calée sur une création réelle.
- Si une préparation ou une génération est en cours, la page « Rushes et montages » l'affiche
  (avec « Annuler ») au lieu de rester muette.

## 0.12

- **Montages enregistrés** : préparer un nouveau rush n'efface plus le montage en cours, il le
  met de côté (moments, sélections et variantes, corrections, effets, découpes, versions de
  travail, 4K, discussion avec Claude). La page « Nouveau rush » (lien « Nouveau rush ou autre
  montage… ») les liste avec « Rouvrir » : le montage revient tel quel en quelques secondes,
  sans rien recalculer, et celui en cours est mis de côté à son tour. « Supprimer » libère la
  place sur le disque.

## 0.11

- **Version 4K depuis l'interface** : le bouton « Télécharger » de la page des moments propose
  maintenant les trois qualités (720p, 1080p, 4K). Si la 4K n'existe pas encore, ou date d'un
  montage précédent, « Créer la version 4K » la fabrique, en prévenant que c'est long (durée
  estimée pour votre ordinateur) ; l'avancement s'affiche sous la sélection et la page reste
  utilisable. Le même bouton est sur la page des téléchargements.

## 0.10

- **Curseur de découpe** : dans chaque moment, la ligne « Découpe » a maintenant un curseur.
  En le faisant glisser, le lecteur montre l'image à cet instant et le mot prononcé s'affiche
  à côté ; ‹ et › avancent ou reculent d'une image pour être précis, puis « Couper ici » coupe
  à l'instant choisi. Les coupes déjà faites sont marquées sur le curseur. Plus besoin de
  cliquer pendant la lecture.

## 0.9

- **Temps restant mieux estimé** pendant la préparation d'un rush et la génération : il part
  de la durée du rush (et du montage pour le rendu), se corrige dès qu'une opération est
  finie, et s'adapte à la vitesse de votre ordinateur, mesurée à chaque préparation ou
  génération (plus juste d'une fois à l'autre).

## 0.8

- **Correction** : après la préparation d'un nouveau rush, la page des moments pouvait montrer
  la version de travail (ou des extraits) d'un rush ou d'un rendu précédent, gardée par le
  navigateur : par exemple une version de 36 s alors que les moments choisis en font 60. La
  page charge maintenant toujours les vidéos à jour.
- **Correction** : un nouveau rush envoyé sous le même nom qu'un ancien (IMG_0001.mov…) est
  bien traité comme un nouveau rush (versions de travail refaites, anciens choix écartés).

## 0.7

- **Barres de progression** avec le temps restant estimé :
  - préparation d'un rush (page « Nouveau rush ») : versions de travail, transcription,
    position du visage, rendu, découpage des moments, et l'ensemble de la préparation ;
  - génération de la version de travail (page des moments) : rendu de la vidéo, mise à jour
    des moments, et l'ensemble de la génération. La page rouverte pendant une génération
    reprend le suivi.

## 0.6

- **Discuter avec Claude** depuis l'interface : nouvelle page (lien « Discuter avec Claude »
  de la page des moments et de « Nouveau rush », ou <http://localhost:8080/chat/>). On y
  demande en français de changer le montage (moments, sous-titres, intro, effets) ; Claude
  modifie les choix et peut générer la version de travail, en montrant ce qu'il fait. La
  conversation est gardée d'une visite à l'autre ; « Nouvelle conversation » repart de zéro.
  Pour sa sécurité, Claude n'y peut que lire le projet, changer les choix de montage et lancer
  les scripts de montage ; le reste se fait avec le lanceur « Claude Code ».

## 0.5

- **Consignes de montage** : sur la page « Nouveau rush », « Choisir un fichier de consignes… »
  prend un fichier texte (.md, .txt) qui décrit le montage voulu. Si Claude Code est connecté
  (lanceur « Claude Code », une fois), il l'applique pendant la préparation : moments gardés,
  intro, effets, corrections. Les choix sont gardés avec le rush et survivent aux mises à jour.
- **Windows** : message clair si Docker Desktop est en mode « conteneurs Windows ».
- **Correction** : la préparation ne s'arrête plus quand aucun visage n'est détecté dans le
  rush (zooms centrés).
- **Correction** : le rendu de la version de travail échouait sur les machines (ou Docker
  Desktop) limitées à 2 ou 3 processeurs ; le message d'erreur affiché est aussi plus clair.
- Tests automatiques de bout en bout (Docker et Podman) à chaque modification et chaque
  semaine, et vérification des lanceurs Windows et macOS.

## 0.4

- **Correction** : la préparation d'un rush s'arrêtait à l'étape « position du visage »
  (erreur « module 'cv2' has no attribute 'CascadeClassifier' »). OpenCV 5, sorti
  récemment, n'a plus ce détecteur : la version 4 est installée à la place, et si la
  détection manque malgré tout, la préparation continue avec des zooms centrés.

## 0.3

- **Les mises à jour se voient tout de suite** : la page des moments d'un rush déjà préparé
  prend désormais l'interface de la version installée (avant, elle gardait celle de la
  version qui l'avait préparé : par exemple sans le lien « Nouveau rush… » de la 0.2).
- **Redémarrage automatique après une mise à jour** : si Auto-montage tournait encore avec
  l'ancienne version, le lanceur le redémarre.
- Le numéro de version installé s'affiche en haut des pages.

## 0.2

- **Nouveau rush sans terminal** : page « Nouveau rush » de l'interface locale. Le rush se
  choisit dans l'explorateur de fichiers (envoi avec barre de progression), puis « Préparer
  ce rush » lance la transcription, la découpe en moments et une première version de travail,
  avec l'avancement à l'écran, avant d'ouvrir la page des moments.
- **Premier montage pour n'importe quel rush** : sans consignes de montage, il garde tous les
  moments sauf les premières prises (avant, les choix écrits pour un autre rush s'appliquaient).
- **Changement de rush propre** : suggestions et sélection de l'ancien rush effacées, ses
  rendus archivés dans `out/archives/`.
- **Correction** : `prepare.sh` et `render.sh` lancés sur la machine (erreur
  « No module named 'imageio_ffmpeg' ») se relancent d'eux-mêmes dans le conteneur.
- **Correction** : la préparation ne s'arrête plus quand un site (dafont…) est
  injoignable depuis le réseau local.
- « Générer la version de travail » fonctionne aussi sur un rush tout juste préparé.

## 0.1

- Premiers installateurs : AppImage (Linux), installateur `.exe` (Windows), `.dmg` (macOS).
- Interface locale (page des moments, version de travail, téléchargements 720p, 1080p, 4K).
- Docker ou Podman sous Linux (avec ou sans root).
