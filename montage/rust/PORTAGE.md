# Portage en Rust (branche `rust`)

Un seul programme, `auto-montage`, remplace les scripts Python et bash de `montage/scripts/`.
Chaque script devient une commande du même nom : `python3 scripts/build_edit.py` devient
`auto-montage build_edit` (`auto-montage scripts/build_edit.py` marche aussi), `./scripts/prepare.sh`
devient `auto-montage prepare`. Les pages (`selection/`, `local/`), le rendu HyperFrames (Node,
Chrome) et les fichiers de `work/` ne changent pas : la version Rust lit et écrit les mêmes fichiers,
de la même forme.

Compilation : `cargo build --release` dans `montage/rust/` ; le programme est
`montage/rust/target/release/auto-montage` (ou `montage/bin/auto-montage` dans une version
installée). Sa racine (`paths::root()`, le dossier `montage/`) est trouvée à partir de son
emplacement ; `AM_ROOT` la force.

## Règles du portage

- **Même comportement.** Mêmes arguments, mêmes fichiers écrits, mêmes messages (en français) : le
  serveur local lit certaines lignes (`@progression`, `@memoire`, `Navigateurs du rendu : N`,
  `Étape 1/4 : …`) et les pages lisent les JSON tels quels.
- **JSON comme Python.** Tout JSON écrit dans un fichier, sur la sortie ou dans une page passe par
  `pyjson` (`dumps`, `dumps_utf8`, `dumps_indent`, ou `util::write_json` qui fait `json.dump`) :
  mêmes séparateurs, flottants écrits comme `repr`, `ensure_ascii` comme le script d'origine.
  `serde_json::Value` garde l'ordre des clés (comme un dict Python) ; un entier Python reste un
  entier (`json!(3)`), un flottant reste un flottant (`util::num(x)` ou `json!(x as f64)`).
- **Arrondis comme Python** : `util::round_to(x, n)` pour `round(x, n)`, `util::round_int(x)` pour
  `round(x)` (moitié au pair) ; `util::fmt_g` pour `%g`.
- **Programmes externes** : `paths::ffmpeg()`, `paths::ffprobe()`, `paths::node()`,
  `paths::whisper_bin("whisper-cli")`. Une commande du projet : `paths::self_exe()` avec son nom en
  premier argument (`Command::new(paths::self_exe()).arg("build_edit")`), jamais `python3` ni `bash`.
- **Trois systèmes.** Le code compile sur Linux, macOS et Windows : rien de propre à Unix sans
  `#[cfg(unix)]` et un équivalent ailleurs ; groupes de processus et signaux par `procs`
  (`new_group`, `kill_group`) ; mémoire par `memoire` ; chemins avec `Path::join`, jamais `/`
  écrit en dur dans un chemin du disque.
- **Commentaires** en français, aussi détaillés que dans le script d'origine ; la docstring du
  script devient le commentaire `//!` en tête du fichier.

## Base commune (`src/`)

| Module | Remplace | Contenu |
| --- | --- | --- |
| `pyjson` | `json.dumps` | JSON identique à Python |
| `util` | — | arrondis, lecture/écriture JSON, fichiers, `fmt_g` |
| `paths` | `ROOT`, `imageio_ffmpeg` | dossiers du projet, programmes externes |
| `procs` | `subprocess`, `os.killpg` | lancement, groupes de processus, codes de sortie |
| `net` | `urllib`, `curl` | téléchargements (proxy, certificats du système) |
| `difflib` | `difflib.SequenceMatcher` | à l'identique (opcodes, ratio) |
| `progress` | `progress.py` | `@progression`, ffmpeg suivi, `exit_reason`, `resources` |
| `moments_lib` | `moments_lib.py` | tout, y compris `voice_filter`, `batterie_warp`, découpage |
| `memoire` | `memoire.py` | besoins, mesures, mémoire libre (cgroup, ou sysinfo hors Linux) |
| `fonts_lib` | `fonts_lib.py` | polices lues dans leurs fichiers (plus de fontconfig) |
| `render_lib` | `render_lib.py` | mesure du texte par HarfBuzz (rustybuzz) : mêmes largeurs que Pillow + raqm |
| `place` | `place.py` | libérer de la place |
