# Nouveautés

Chaque version publiée reprend sa section dans ses notes (onglet Releases). Une nouvelle
section, le numéro dans `VERSION` et le tag `v<numéro>` poussé publient la version avec ses
installateurs (voir `packaging/README.md`).

## 0.67

- **Moments changés depuis la version de travail** : sur la page des moments, un moment dont
  les réglages ne sont plus ceux de la version de travail affichée porte une pastille jaune
  « Modifié » ; au survol, elle dit de quelle version de travail (date du rendu) et ce qui a
  changé (sous-titres, découpe, effets, volume, cadrage…, moment ajouté ou retiré). Un point jaune
  le marque aussi sur la frise du rush, et le haut de la page compte ces moments. « Générer la version de travail » les remet à jour.

## 0.66

- **Frise du moment** : au lieu des images, elle montre les **effets** du moment (une bande de
  couleur à leur nom, clic : leurs réglages) et ses **sous-titres** mot à mot (un clic sur un mot
  y place la tête de lecture), quand il y en a ; en bas, une **ligne de coupe** où se coupent et
  se gardent ou se retirent les parties. Sans effet ni sous-titre, il ne reste que cette ligne.

## 0.65

- **Page des moments façon logiciel de montage** (CapCut), en sombre : la barre du haut garde
  la durée gardée, la variante et « Générer la version de travail » ; au milieu, le lecteur et
  un **inspecteur** à droite (le moment choisi : garder, texte des sous-titres, fusion, effets ;
  onglet « Tout le montage » pour le style des sous-titres, les zooms, le son et les effets
  proposés) ; en bas, les frises.
- **Frise du rush** : une seule ligne qui défile de gauche à droite, chaque moment avec ses
  images, une case pour le garder ou le retirer, ses parties retirées hachurées, et dessous la
  **forme d'onde** de la voix (verte pour ce qui est gardé). Un curseur zoome la frise ; on
  glisse la tête de lecture sur la règle. Le moment choisi a un contour blanc et deux poignées
  pour raccourcir son début ou sa fin. Barre d'outils : Couper, Scinder, Fusionner, Garder,
  Retirer, Corriger.
- **Frise du moment**, sous celle du rush quand un moment est choisi : ses images seulement, à
  zoomer jusqu'à ×16, pour placer la tête de lecture à l'image près, couper (« Couper ici »),
  garder ou retirer chaque partie et voir ses effets en fins repères de couleur.
- **Plusieurs moments d'un coup** : Maj + clic sélectionne une suite de moments, Ctrl + clic
  (⌘ sur Mac) en ajoute un ; on les garde, les retire ou les fusionne en un seul.
- **Clavier** : Espace lit ou met en pause, ← → avancent d'une image, Maj + ← → passent au
  moment précédent ou suivant, Échap ferme le moment.
- **Nouvel effet « Zoom libre »** : un zoom progressif d'un cadre à un autre, comme les images
  clés de CapCut. Le début et la fin se placent à la tête de lecture (« Ici »), et chaque cadre
  se règle en zoomant l'image du lecteur (molette, pincement ou curseur) et en la faisant
  glisser ; les deux cadres sont des losanges jaunes sur la frise du moment.
- En Dérushage, pas d'effets : la frise du moment, le texte des sous-titres (si la case est
  cochée) et les actions de découpe restent.

## 0.64

- **Deux modes, au choix dans le bandeau du haut** : « Dérushage » et « Auto-montage ».
  En **Dérushage**, Auto-montage ne sert qu'à découper le rush en moments : cocher les moments
  à garder, couper, scinder ou fusionner, puis générer la version de travail, qui les met bout à
  bout tels quels (ni zooms, ni effets, ni réglages du son). Les sous-titres restent, avec une
  case « Sous-titres dans la version de travail » (cochée au départ) pour les retirer, « Corriger »
  et « Supprimer les sous-titres » sur chaque moment, et leur style. Les effets, les
  consignes, le style et tout ce qui demande Claude disparaissent des pages, et les pages
  « Discuter avec Claude » et « Connexion à Claude » du menu. **Auto-montage** garde toutes les
  fonctions, comme avant. Le choix vaut pour tous les montages ; les effets déjà placés sont
  gardés et reviennent en repassant en Auto-montage.

## 0.63

- **La barre de progression ne recule plus** : elle avançait d'après le temps restant estimé, et
  reculait quand cette estimation s'allongeait. Elle suit maintenant la part du rush déjà
  traitée par l'étape en cours (par exemple 2 min sur 6 min de vidéo transcrites) : elle ne fait
  qu'avancer. Le temps restant affiché à côté reste une estimation.

## 0.62

- **Lecteur toujours visible sur la page des moments** : la vidéo, ses onglets (version de
  travail, moment, plein écran, télécharger) et les boutons de la synthèse restent en haut de
  l'écran pendant qu'on descend dans la liste ; dans l'interface locale, ils ne passent plus sous
  le bandeau du menu. Sur un écran étroit, le lecteur reste en haut en plus petit (il n'apparaissait
  plus du tout auparavant) et la synthèse défile avec la liste.
- **Bouton « haut de la page »** : une flèche en bas à droite ramène tout en haut d'un clic.

## 0.61

- **Plusieurs personnes à l'image** : les zooms gardent maintenant tout le monde dans l'image.
  Quand deux personnes sont filmées ensemble (interview, scène, écran partagé), le zoom se centre
  sur elles deux et s'arrête avant de couper l'une d'elles ; avec une seule personne, les zooms
  restent comme avant. Les personnes au fond ou dans le public ne comptent pas.
- **Visages mieux trouvés**, même petits ou quand la caméra bouge. La page des moments montre
  le même cadrage que la vidéo finale, qui suit les visages pendant tout le moment.
- **Cadrage par moment** : sur la page des moments, un moment où l'on voit plusieurs personnes
  propose « Cadrage : Tout le monde » ou la photo de chacune. En choisissant une personne, l'image
  se resserre sur elle (plan rapproché), dans l'aperçu comme dans la vidéo. Une même personne est
  reconnue d'un plan de caméra à l'autre.
- **Écran partagé et incrustation** : quand chaque personne est dans son propre cadre (visio à
  côté de la scène, image dans l'image), le plan rapproché sur une personne remplit l'écran avec
  son cadre, sans montrer la séparation ni l'autre image.

## 0.60

- **La préparation en pause n'est plus perdue à la mise à jour** : mettre à jour (ou réinitialiser)
  Auto-montage pendant qu'une préparation était en pause effaçait la pause et ce qui était déjà
  fait (version de travail en cours, positions du visage) : il fallait tout recommencer. Elle est
  maintenant gardée et se reprend après le redémarrage. Cette mise à jour-ci (depuis la 0.59) est
  encore faite par l'ancien programme : finissez la préparation en pause avant de l'installer,
  ou copiez de côté `montage/work/tache.json` et le dossier `montage/work/reprise`, puis
  remettez-les une fois la mise à jour faite.

## 0.59

- **Rendu des longs montages sans remplir le disque** : HyperFrames gardait les images extraites
  des plans de tout le montage dans un dossier temporaire caché, vidé seulement au bout d'une
  heure (plus de 13 Go pour un montage de 22 minutes) : le disque se remplissait et le rendu
  échouait (« Video extraction failed »). Les images de chaque partie sont maintenant effacées à
  la fin de la partie, et les parties sont raccourcies quand le disque est presque plein. S'il
  n'y a vraiment pas assez de place, le rendu le dit tout de suite.
- Les images laissées par un rendu précédent sont effacées au démarrage d'Auto-montage et au
  début de chaque rendu ; elles apparaissent aussi dans « Libérer de la place ».

## 0.58

- **Rendu par parties** : un montage de plus de 3 minutes est rendu par parties d'environ
  2 minutes, mises bout à bout ensuite (image identique, son sans décalage). La mémoire du rendu
  ne grandit plus avec la longueur du montage : sur un montage de 5 minutes, 5,8 Go au lieu de
  7 Go, et pas plus pour un montage plus long. Un rendu interrompu (pause, arrêt, panne)
  reprend après la dernière partie finie au lieu de tout refaire.
- **Mémoire utilisée au mieux** : le rendu ouvre autant de navigateurs que 80 % de la mémoire
  libre le permet (un par cœur au plus), 4K comprise, d'après ce que les rendus précédents ont
  vraiment pris sur la machine.
- **Reprendre plutôt que tout refaire** : choisir le rush d'une préparation en pause propose
  « Reprendre la préparation » ; avant, « Préparer » abandonnait la préparation en pause et
  effaçait ce qui était fait.
- Les mesures de mémoire de la machine restent quand on change de rush (elles partaient avec le
  montage mis de côté).

## 0.57

- **Mémoire** : avant de préparer un rush, la page **Rushes et montages** dit combien de mémoire la
  préparation demandera d'après la vidéo (taille de l'image, durée), étape par étape, et combien il
  en reste de libre. Si c'est juste ou insuffisant, un avertissement explique quoi faire (fermer
  d'autres programmes, donner plus de mémoire à Docker). La vérification est refaite au début de
  chaque étape, et un avertissement apparaît si la mémoire s'épuise en cours de route. La création
  de la 4K, la plus gourmande, prévient aussi.
- **Pause et reprise** : une préparation peut être mise en pause à tout moment (« Mettre en
  pause »), puis reprise plus tard là où elle en était (« Reprendre la préparation »), même après
  avoir fermé Auto-montage, Docker ou éteint l'ordinateur. Une préparation coupée par un arrêt ou
  arrêtée par une erreur (mémoire, place sur le disque) se reprend de la même façon, sans tout
  refaire : la version de travail repart de la dernière minute faite, la transcription des passages
  pas encore transcrits, la détection du visage de sa dernière position.
- **Abandonner en gardant une partie** : « Abandonner… » liste ce qui est déjà fait (version de
  travail, son et transcription, position du visage, extraits de la page des moments) avec la place
  que chaque partie prend. Tout est coché pour être effacé ; une partie décochée est gardée, et la
  prochaine préparation du même rush la reprend sans la refaire.
- **Mémoire apprise sur la machine** : chaque préparation mesure la mémoire vraiment utilisée par
  chaque étape. Les estimations suivantes partent de ces mesures, rapportées à la taille de la
  nouvelle vidéo, et la page dit pour chaque étape si le chiffre vient de mesures ou d'une
  estimation.
- **Rendu des longues vidéos** : le rendu de la version de travail d'un long rush pouvait épuiser la
  mémoire et s'arrêter (« code -9 »). HyperFrames lisait tous les plans en même temps (150 plans :
  plus de 10 Go) ; il en lit maintenant 4 à la fois (2 en 4K), sous 1 Go. Ensuite, chaque
  navigateur du rendu prend plus d'un gigaoctet : le rendu en ouvre moins de 4 quand la mémoire
  libre ne suffit pas (plus lent, mais il va au bout), et donne plus de temps à la page d'un long
  montage pour être prête.
- **Avancement détaillé** : chaque étape de la préparation montre où elle en est, en pourcentage et
  en temps de vidéo traité (« Transcription : 43 % · 2 min 41 s sur 6 min 17 s de vidéo »).

## 0.56

- **Lignes des sous-titres** : dans « Style des sous-titres » de la page des moments, et sur la page
  **Rushes et montages**, choisissez le nombre de lignes de chaque sous-titre (1, 2 ou 3) et la
  taille de chaque ligne par rapport à la première (de 50 à 200 %), par exemple une deuxième ligne
  plus grosse pour faire ressortir la fin de la phrase. Le texte est coupé pour que les lignes
  gardent des largeurs proches, et le nombre de mots par sous-titre suit le nombre de lignes
  (3 sur une ligne, 6 sur deux, 9 sur trois). Un sous-titre trop haut pour l'image est remonté.

## 0.55

- **Libérer de la place** : nouvelle section en bas de la page **Rushes et montages**. Elle liste
  avec leur taille les montages enregistrés, les vidéos envoyées (copies gardées par
  Auto-montage), la source 4K et les copies laissées par le dernier rendu, les sauvegardes et
  les envois coupés. On coche ce qui peut partir, puis « Supprimer la sélection ». Les vidéos
  encore utiles à un montage sont signalées avant suppression.
- Un envoi coupé (page fermée, Docker arrêté) ne laisse plus de fichier caché de plusieurs Go,
  et les copies de vidéos du dernier rendu ne gardent plus sur le disque les vidéos d'un
  montage mis de côté.
- **Nettoyer Docker** : nouveau choix dans le menu de l'application (Mac, Windows, Linux). Il
  supprime les anciennes versions d'Auto-montage gardées par Docker (une par mise à jour, plusieurs
  Go chacune) et son cache, sans toucher à la connexion à Claude, à Whisper ni aux montages. Les
  lanceurs effacent aussi d'eux-mêmes l'ancienne version après chaque mise à jour.
- Le message « Pas assez de place sur le disque » dit maintenant combien il faut, combien il
  reste et ce qui prend le plus de place.

## 0.54

- **Vidéos en anglais** : sur la page **Rushes et montages**, l'étape « Préparer » propose
  maintenant la langue parlée dans la vidéo (français par défaut, ou anglais). La transcription et
  les consignes appliquées par Claude suivent ce choix, qui est gardé pour les rushes suivants.
- **Plusieurs micros** : quand une vidéo a plusieurs pistes son (un micro par personne,
  enregistreur à part…), toutes les pistes sont maintenant gardées et mélangées. Avant, seule la
  première était prise : une personne enregistrée sur la deuxième piste disparaissait de la
  transcription et du montage.
- Une vidéo sans son est refusée tout de suite avec un message clair, au lieu d'une erreur.

## 0.53

- **Préparation beaucoup plus rapide** : la transcription, l'étape la plus longue, est environ
  5 fois plus rapide (un rush de 6 min transcrit en 7 min 30 au lieu de 37 min, mesuré sur
  4 cœurs). Les phrases sont maintenant transcrites ensemble par morceaux d'une demi-minute au
  lieu d'une par une, ce qui donne aussi un texte plus juste (noms propres, ponctuation) et moins
  de phrases inventées dans les silences. Les hésitations isolées (« euh… ») restent transcrites,
  pour repérer les ratés.

## 0.52

- **Journal plus clair quand une tâche s'arrête** (page **Journal**) : pendant les étapes
  longues, la place libre sur le disque et la mémoire disponible sont notées toutes les
  30 secondes. Si ffmpeg s'arrête, le journal dit pourquoi (par exemple « tué de force, le plus
  souvent par manque de mémoire ») et montre ses derniers messages. Une tâche coupée par l'arrêt
  d'Auto-montage, de Docker ou de l'ordinateur est marquée « interrompu » au redémarrage, avec
  l'heure de sa dernière écriture. Si le journal ne peut plus être écrit (disque plein), la page
  le signale.

## 0.51

- **Réinitialiser Auto-montage** (page **Mises à jour**) : si Claude Code, lancé dans le dossier
  d'Auto-montage, a modifié le programme et que quelque chose ne marche plus, un bouton
  retélécharge depuis GitHub les fichiers de la version installée et remet comme à l'origine
  ceux qui ont été modifiés ou effacés, puis Auto-montage redémarre. Une confirmation est
  demandée avant ; vos montages, rushes, rendus, sons, consignes et réglages ne sont pas
  touchés, et une copie des fichiers remplacés est gardée dans `montage/work/sauvegardes/`.
- Le dépôt étant public, la page **Mises à jour** ne demande plus de jeton GitHub.

## 0.50

- **Première version du dépôt public** : Auto-montage est maintenant publié sur
  <https://github.com/RobinKD/Auto-montage> sous licence GNU GPL version 3, avec les
  installateurs Windows, macOS et Linux joints à chaque version. Les fonctionnalités sont celles
  de la 0.43.
- Les installations antérieures à la 0.40 ne peuvent pas se mettre à jour depuis la page
  **Mises à jour** : il faut réinstaller avec les installateurs (montages et réglages gardés).

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
