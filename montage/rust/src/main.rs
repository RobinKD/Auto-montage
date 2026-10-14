//! Auto-montage, version native : un seul programme pour toutes les étapes (préparation d'un rush,
//! montage, rendu, interface locale), qui remplace les scripts Python et bash de `scripts/`.
//!
//! Usage : auto-montage <commande> [arguments…]   (`auto-montage aide` pour la liste)
//! Chaque commande garde le nom du script qu'elle remplace : `python3 scripts/build_edit.py`
//! devient `auto-montage build_edit` (les formes `scripts/build_edit.py` et `build_edit.py` sont
//! aussi acceptées). Lancé sous le nom `ffmpeg_limite`, le programme fait le ffmpeg donné à
//! HyperFrames (extractions d'images limitées, voir ffmpeg_limite.rs).

mod analyze_sound;
mod analyze_style;
mod build_edit;
mod claude_tasks;
mod decoupage;
mod detect_face;
mod diarize;
mod difflib;
mod export;
mod ffmpeg_limite;
mod fonts_lib;
mod join_rushes;
mod local_server;
mod look;
mod loudness;
mod make_downloads;
mod make_hd;
mod make_moments;
mod make_sfx;
mod mcp;
mod memoire;
mod moments_lib;
mod net;
mod paths;
mod place;
mod prepare;
mod procs;
mod progress;
mod project;
mod pyjson;
mod lanceur;
mod relance;
mod render;
mod render_hyperframes;
mod render_lib;
mod setup;
mod subtitles;
mod transcribe_segments;
mod update;
mod use_selection;
mod util;
mod version_travail;

type Cmd = fn(&[String]) -> i32;

/// (commande, fonction, description)
const COMMANDS: &[(&str, Cmd, &str)] = &[
    ("start", lanceur::start, "installe ce qui manque, lance l'interface et l'ouvre dans le navigateur"),
    ("stop", lanceur::stop, "arrête l'interface"),
    ("prepare", prepare::main, "prépare un rush (remplace prepare.sh)"),
    ("render", render::main, "rendu apercu|final (remplace render.sh)"),
    ("local_server", local_server::main, "interface web locale (port 8080)"),
    ("make_moments", make_moments::main, "page des moments"),
    ("build_edit", build_edit::main, "montage : src/data/edit.json"),
    ("use_selection", use_selection::main, "choix de la page -> work/selection.json"),
    ("decoupage", decoupage::main, "scinder ou fusionner des moments"),
    ("loudness", loudness::main, "volume perçu d'un fichier (LUFS), normalisation"),
    ("subtitles", subtitles::main, "sous-titres du montage en fichiers .srt et .vtt (out/)"),
    ("mcp", mcp::main, "serveur MCP : Claude pilote le montage (claude mcp add auto-montage -- ./am mcp)"),
    ("export", export::main, "copies pour les plateformes (YouTube, TikTok, Instagram…) : out/export/"),
    ("make_downloads", make_downloads::main, "page des téléchargements"),
    ("make_hd", make_hd::main, "source 4K des images gardées"),
    ("project", project::main, "montages enregistrés (save, open, delete)"),
    ("place", place::main, "libérer de la place"),
    ("memoire", memoire::main, "mémoire nécessaire à la préparation"),
    ("progress", progress::main, "ffmpeg suivi en barre de progression"),
    ("fonts_lib", fonts_lib::main, "polices des sous-titres"),
    ("voix", moments_lib::voix_main, "voix modifiées (exemples de tests/make_voice_demo.py)"),
    ("version_travail", version_travail::main, "version de travail 1080p"),
    ("join_rushes", join_rushes::main, "assemble plusieurs vidéos d'un rush"),
    ("transcribe_segments", transcribe_segments::main, "transcription Whisper par segment"),
    ("detect_face", detect_face::main, "visages et cadrage"),
    ("diarize", diarize::main, "qui parle : locuteurs et personne à l'image de chacun"),
    ("make_sfx", make_sfx::main, "bruitages par défaut"),
    ("analyze_sound", analyze_sound::main, "analyse du son de la voix"),
    ("analyze_style", analyze_style::main, "analyse des vidéos d'exemple"),
    ("render_hyperframes", render_hyperframes::main, "rendu HyperFrames du montage"),
    ("update", update::main, "mises à jour (check, apply, reset)"),
    ("setup", setup::main, "installe ce qui manque (paquets Node, Whisper, police, modèles)"),
    ("setup_whisper", setup::setup_whisper, "whisper.cpp et ses modèles"),
    ("fetch_fonts", setup::fetch_fonts, "police Oliver"),
    ("fetch_ffmpeg", setup::fetch_ffmpeg, "ffmpeg livré avec Auto-montage (montage/bin/)"),
    ("fetch_face_model", setup::fetch_face_model, "modèles des visages"),
    ("fetch_node", setup::fetch_node, "Node.js autonome si la machine n'en a pas"),
    ("claude_code", setup::claude_code, "Claude Code et son plugin HyperFrames"),
    ("ensure_browser", setup::ensure_browser, "navigateur de rendu"),
    ("check_network", setup::check_network, "domaines réseau nécessaires"),
    ("apply_instructions", claude_tasks::apply_instructions, "consignes de montage appliquées par Claude"),
    ("style_claude", claude_tasks::style_claude, "analyse du style par Claude"),
    ("ffmpeg_limite", ffmpeg_limite::main, "ffmpeg donné à HyperFrames"),
    ("version", version, "numéro de version de ce programme"),
];

/// Numéro de version compilé dans le programme (fichier VERSION du dépôt, build.rs).
pub const VERSION: &str = env!("AM_VERSION");

fn version(_: &[String]) -> i32 {
    println!("{VERSION}");
    0
}

fn usage() -> i32 {
    eprintln!("Auto-montage {} : auto-montage <commande> [arguments…]\n", env!("AM_VERSION"));
    for (name, _, what) in COMMANDS {
        eprintln!("  {name:22} {what}");
    }
    2
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    paths::extend_path();
    // Lancé sous un autre nom (copie ou lien « ffmpeg_limite ») : cette commande-là.
    let stem = std::path::Path::new(&args[0]).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if stem == "ffmpeg_limite" {
        std::process::exit(ffmpeg_limite::main(&args[1..]));
    }
    let Some(first) = args.get(1) else { std::process::exit(usage()) };
    let name = first.trim_start_matches("./").trim_start_matches("scripts/");
    let name = name.strip_suffix(".py").or_else(|| name.strip_suffix(".sh")).unwrap_or(name);
    let code = match COMMANDS.iter().find(|(n, _, _)| *n == name) {
        Some((_, f, _)) => f(&args[2..]),
        None => usage(),
    };
    std::process::exit(code);
}
