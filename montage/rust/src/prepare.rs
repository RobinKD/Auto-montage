//! Prépare un rush : versions de travail, transcription, visage, montage.
//!
//! Usage : auto-montage prepare <fichier du rush dans public/rushes/, ex. MonRush.mov>
//!         auto-montage prepare <vidéo 1> <vidéo 2> […]   plusieurs vidéos pour un même rush,
//!         assemblées dans l'ordre (join_rushes) puis préparées comme un seul rush
//!         auto-montage prepare --langue en <fichier…>   rush en anglais (français par défaut ;
//!         le choix est gardé pour les préparations suivantes : work/langue.txt)
//!         auto-montage prepare --reprendre <fichier…>   reprend une préparation interrompue (pause,
//!         arrêt, panne) là où elle en était : morceaux de la version de travail (work/reprise/),
//!         morceaux transcrits (work/seg/), positions du visage déjà trouvées. Sans cette option
//!         (ni AM_REPRISE=1, mis par l'interface locale), tout est refait.
//!
//! Chaque étape est une commande de ce programme lancée comme processus enfant (join_rushes,
//! project save, memoire, version_travail, setup_whisper, transcribe_segments, detect_face,
//! fetch_fonts, make_sfx, build_edit…) : le groupe de processus de la tâche (annulation, pause) et
//! la mesure de la mémoire de l'interface locale les voient comme avant. Une étape qui échoue
//! arrête la préparation avec son code (comme `set -e` du script d'origine).

use crate::moments_lib::{audio_map_of, audio_streams, display_size, frame_size, language, language_label, work_size};
use crate::{paths, procs, progress, util};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

/// Code de sortie d'une étape comme le shell (tuée par le signal n : 128 + n).
fn shell_code(code: i32) -> i32 {
    if code < 0 {
        128 - code
    } else {
        code
    }
}

/// Lance `auto-montage <args…>` depuis la racine du projet (sortie héritée) ; code à la façon du shell.
fn step(args: &[&str]) -> i32 {
    match Command::new(paths::self_exe()).args(args).current_dir(paths::root()).status() {
        Ok(s) => shell_code(procs::py_code(&s)),
        Err(e) => {
            eprintln!("Impossible de lancer {} : {e}", args.first().unwrap_or(&""));
            127
        }
    }
}

fn usage() -> i32 {
    eprintln!("Usage : auto-montage prepare <fichier du rush dans public/rushes/> [<vidéo suivante>…]");
    1
}

/// Clé du fichier du rush : chemin, taille et date (comme `stat -c '%s %Y'`).
fn file_key(rush: &str) -> String {
    let p = paths::root().join(rush);
    let size = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    let mtime = util::mtime(&p).unwrap_or(0.0).floor() as i64;
    format!("{rush} {size} {mtime}")
}

macro_rules! check {
    ($code:expr) => {{
        let c = $code;
        if c != 0 {
            return c;
        }
    }};
}

pub fn main(args: &[String]) -> i32 {
    let root = paths::root();
    if let Err(e) = std::env::set_current_dir(root) {
        eprintln!("Dossier du projet inaccessible ({}) : {e}", paths::display(root));
        return 1;
    }
    let work = paths::work();
    util::mkdirs(&work);
    let mut args: Vec<String> = args.to_vec();
    if args.first().map(String::as_str) == Some("--reprendre") {
        std::env::set_var("AM_REPRISE", "1");
        args.remove(0);
    }
    if args.first().map(String::as_str) == Some("--langue") {
        let lang = args.get(1).cloned().unwrap_or_default();
        if language_label(&lang).is_none() {
            eprintln!("Langue inconnue : {lang} (fr ou en)");
            return 1;
        }
        if let Err(e) = fs::write(work.join("langue.txt"), format!("{lang}\n")) {
            eprintln!("work/langue.txt : {e}");
            return 1;
        }
        args.drain(..2);
    }
    if args.is_empty() {
        return usage();
    }
    let mut name = args[0].clone();
    if args.len() > 1 {
        let refs: Vec<&str> = std::iter::once("join_rushes").chain(args.iter().map(String::as_str)).collect();
        check!(step(&refs));
        name = crate::join_rushes::joined_name(&args);
    }
    let base = Path::new(&name).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    // Chemin relatif à la racine, écrit tel quel dans work/rush.txt (lu par les autres commandes).
    let rush = format!("public/rushes/{base}");
    let rush_path = root.join("public").join("rushes").join(&base);
    if !rush_path.is_file() {
        eprintln!("Rush introuvable : {rush}");
        return 1;
    }

    // Son du rush : sa piste son, ou toutes ses pistes mélangées (un micro par personne sur des
    // pistes séparées) ; repris par build_edit (moments_lib::audio_map).
    let streams = audio_streams(&rush_path);
    if streams.len() > 1 {
        eprintln!("{} pistes son mélangées", streams.len());
    }
    let Some(audio_map) = audio_map_of(&streams, "son") else {
        eprintln!("Cette vidéo n'a pas de son (ou un son illisible) : Auto-montage découpe le rush d'après la parole, il lui faut le son.");
        return 1;
    };

    // Domaines réseau de l'environnement cloud (session Claude Code sur le web) : signalés seulement.
    if std::env::var("CLAUDE_CODE_REMOTE").as_deref() == Ok("true") {
        let _ = step(&["check_network"]);
    }
    let ffmpeg = paths::display(paths::ffmpeg());

    // Rush en cours : lu par les autres commandes. Quand il change, le montage de l'ancien rush
    // (versions de travail, transcription, page des moments, choix, rendus) est mis de côté dans
    // work/projets/ (project) : la page « Nouveau rush » le rouvre sans rien recalculer.
    // Un autre fichier envoyé sous le même nom (IMG_0001.mov…) compte aussi comme un nouveau rush :
    // le nom, la taille et la date du fichier sont comparés à ceux de la dernière préparation.
    let old = util::read_text(work.join("rush.txt")).unwrap_or_default();
    let key = file_key(&rush);
    let old_key = util::read_text(work.join("rush_key.txt")).unwrap_or_default();
    if old != rush || (!old_key.is_empty() && old_key != key) {
        check!(step(&["project", "save"]));
        util::mkdirs(&work);
    }
    if fs::write(work.join("rush.txt"), format!("{rush}\n")).is_err() || fs::write(work.join("rush_key.txt"), format!("{key}\n")).is_err() {
        eprintln!("Impossible d'écrire dans work/");
        return 1;
    }
    // Préparation reprise : ce qui est déjà fait est gardé ; sinon, restes d'une préparation
    // interrompue effacés (tout est refait).
    let reprise = std::env::var("AM_REPRISE").as_deref() == Ok("1");
    if !reprise {
        util::remove_all(work.join("reprise"));
    }
    // Mémoire nécessaire à chaque étape d'après la vidéo, comparée à la mémoire libre (avertissement).
    let _ = step(&["memoire", &rush]);
    // Langue du rush (transcription, consignes appliquées par Claude) : celle choisie pour cette préparation.
    let langue = language(&work);
    let _ = fs::write(work.join("langue_rush.txt"), format!("{langue}\n"));

    progress::say("Étape 1/4 : copie du rush en 1080p (quelques minutes)");
    // 1. Version de travail 1080p en MP4 (analyse du visage, extraits, rendu HyperFrames), au format
    //    du rush, petit côté à 1080 : 1080x1920 en 9:16, 1920x1080 en 16:9, 1440x1080 en 4:3…
    //    (moments_lib::work_size ; repris par tout le montage).
    //    (Écrite sous un nom provisoire .part puis renommée : une préparation interrompue ne laisse
    //    pas un fichier incomplet que la suivante prendrait pour bon.)
    let size = display_size(&rush_path).map(|(w, h)| work_size(w, h, 1080)).unwrap_or((1080, 1920));
    let work_video = paths::rushes().join("rush_1080.mp4");
    // Version de travail d'une autre taille (même rush préparé avant la 0.43, étiré en 1080x1920) : refaite.
    if work_video.is_file() && frame_size(root) != size {
        let _ = fs::remove_file(&work_video);
    }
    //    Encodée par morceaux d'une minute (version_travail) : une reprise repart du premier
    //    morceau manquant.
    if !work_video.is_file() {
        check!(step(&["version_travail", &rush]));
    }

    let label = language_label(&langue).unwrap_or("Français").to_lowercase();
    progress::say(&format!("Étape 2/4 : transcription en {label} (Whisper, le plus long : environ la durée du rush)"));
    // 2. Audio 16 kHz mono, segments de parole (VAD Silero) et transcription par segment.
    //    (Reprise : son et segments déjà faits gardés, morceaux déjà transcrits aussi.)
    check!(step(&["setup_whisper"]));
    let wav = work.join("rush_16k.wav");
    if !reprise || !wav.is_file() {
        let part = work.join("rush_16k.part.wav");
        let mut cmd: Vec<String> = vec![ffmpeg.clone(), "-v".into(), "error".into(), "-y".into(), "-i".into(), rush.clone()];
        cmd.extend(audio_map.iter().cloned());
        cmd.extend(["-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"].iter().map(|s| s.to_string()));
        cmd.push(paths::display(&part));
        check!(progress::ffmpeg("Extraction du son", &cmd, None, 0.0, None));
        if let Err(e) = fs::rename(&part, &wav) {
            eprintln!("work/rush_16k.wav : {e}");
            return 1;
        }
        let _ = fs::remove_file(work.join("vad.txt"));
    }
    let vad = work.join("vad.txt");
    if !reprise || !vad.is_file() {
        progress::say("@memoire Détection de la parole"); // mémoire mesurée par l'interface locale (sans barre)
        let part = work.join("vad.part.txt");
        let out = match fs::File::create(&part) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("work/vad.part.txt : {e}");
                return 1;
            }
        };
        let model = paths::whisper_dir().join("ggml-silero-v5.1.2.bin");
        let status = Command::new(paths::whisper_bin("vad-speech-segments"))
            .arg("-f")
            .arg(&wav)
            .arg("-vm")
            .arg(&model)
            .args(["-vsd", "150", "-vp", "40", "-np"])
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(Stdio::null())
            .status();
        let code = match status {
            Ok(s) => shell_code(procs::py_code(&s)),
            Err(e) => {
                eprintln!("Impossible de lancer vad-speech-segments : {e}");
                127
            }
        };
        check!(code);
        if let Err(e) = fs::rename(&part, &vad) {
            eprintln!("work/vad.txt : {e}");
            return 1;
        }
    }
    check!(step(&["transcribe_segments"]));

    progress::say("Étape 3/4 : position du visage");
    // 3. Visages et cadrage (YuNet) : src/data/face.json.
    util::mkdirs(work.join("models"));
    if step(&["fetch_face_model"]) != 0 {
        progress::say("Modèle YuNet indisponible : visages non détectés.");
    }
    check!(step(&["detect_face"]));

    progress::say("Étape 4/4 : bruitages et montage");
    progress::say("@memoire Bruitages et montage");
    // 4. Police Oliver, bruitages provisoires et montage.
    check!(step(&["fetch_fonts"]));
    check!(step(&["make_sfx"]));
    check!(step(&["build_edit"]));
    0
}
