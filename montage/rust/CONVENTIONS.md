# Programme Auto-montage (Rust)

Un seul programme, `auto-montage`, fait toutes les étapes : `auto-montage <commande>` (`./am
<commande>` depuis `montage/`, `auto-montage aide` pour la liste). Les pages (`selection/`,
`local/`), le rendu HyperFrames (Node, Chrome) et les fichiers de `work/` sont lus et écrits par
ses commandes.

Compilation : `cargo build --release` dans `montage/rust/` ; le programme est
`montage/rust/target/release/auto-montage` (ou `montage/bin/auto-montage` dans une version
installée). Sa racine (`paths::root()`, le dossier `montage/`) est trouvée à partir de son
emplacement ; `AM_ROOT` la force.

## Règles

- **Messages lus par le serveur local** : `@progression`, `@memoire`, `Navigateurs du rendu : N`,
  `Étape 1/4 : …` gardent leur forme ; les pages lisent les JSON tels quels.
- **JSON** écrit par `pyjson` (`dumps`, `dumps_utf8`, `dumps_indent`, ou `util::write_json`) :
  ordre des clés gardé, entiers et flottants distingués (`json!(3)`, `util::num(x)`), `<` échappé
  dans ce qui part dans une page.
- **Arrondis** : `util::round_to(x, n)`, `util::round_int(x)` (moitié au pair) ; `util::fmt_g` pour `%g`.
- **Programmes externes** : `paths::ffmpeg()`, `paths::ffprobe()`, `paths::node()`,
  `paths::whisper_bin("whisper-cli")`. Une commande du projet : `paths::self_exe()` avec son nom en
  premier argument (`Command::new(paths::self_exe()).arg("build_edit")`).
- **Trois systèmes.** Le code compile sur Linux, macOS et Windows : rien de propre à Unix sans
  `#[cfg(unix)]` et un équivalent ailleurs ; groupes de processus et signaux par `procs`
  (`new_group`, `kill_group`) ; mémoire par `memoire` ; chemins avec `Path::join`, jamais `/`
  écrit en dur dans un chemin du disque.
- **Commentaires** en français ; le rôle d'une commande dans le commentaire `//!` en tête du fichier.

## Base commune (`src/`)

| Module | Contenu |
| --- | --- |
| `pyjson` | écriture du JSON (séparateurs, flottants, `ensure_ascii`) |
| `util` | arrondis, lecture/écriture JSON, fichiers, `fmt_g` |
| `paths` | dossiers du projet, programmes externes |
| `procs` | lancement, groupes de processus, codes de sortie |
| `net` | téléchargements (proxy, certificats du système) |
| `difflib` | comparaison de suites de mots (opcodes, ratio) |
| `progress` | `@progression`, ffmpeg suivi, `exit_reason`, `resources` |
| `moments_lib` | moments, effets, voix (`voice_filter`, `batterie_warp`), découpage |
| `memoire` | besoins, mesures, mémoire libre (cgroup, ou sysinfo hors Linux) |
| `fonts_lib` | polices lues dans leurs fichiers |
| `render_lib` | mesure du texte par HarfBuzz (rustybuzz) |
| `place` | libérer de la place |
