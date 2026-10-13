//! Sous-titres du montage en fichiers : SubRip (.srt) et WebVTT (.vtt), pour YouTube, les
//! lecteurs vidéo, l'accessibilité, ou pour les incruster ailleurs (formats d'échange repris de
//! la liste de FilmCraft : SRT et WebVTT à l'image près).
//!
//! Source : les sous-titres de src/data/edit.json (build_edit : mots corrigés sur la page,
//! groupés comme à l'écran, deux lignes « top » / « bottom »), texte tel que prononcé (sans les
//! majuscules du style). Un sous-titre reste affiché jusqu'au suivant à l'écran ; dans le fichier,
//! il s'arrête au plus 1,2 s après le début de son dernier mot (pas de texte sur un long silence).
//!
//! Sorties : out/montage.srt et out/montage.vtt (écrites après chaque rendu par render, aussi par
//! « auto-montage subtitles »), proposées sur la page « Téléchargements » et dans le menu
//! « Télécharger » de l'interface locale.

use crate::paths;
use crate::util;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// Formats proposés : (extension, nom, type, note).
pub const FORMATS: [(&str, &str, &str, &str); 2] = [
    ("srt", "SRT", "application/x-subrip", "SubRip : YouTube, Facebook, lecteurs vidéo, logiciels de montage."),
    ("vtt", "WebVTT", "text/vtt", "WebVTT : pages web, Vimeo, LinkedIn."),
];

/// Nombre d'entrées d'un fichier écrit (lignes « --> »).
pub fn count(path: &std::path::Path) -> usize {
    fs::read_to_string(path).map(|t| t.lines().filter(|l| l.contains(" --> ")).count()).unwrap_or(0)
}

/// Une entrée : début, fin (secondes du montage), lignes de texte.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub lines: Vec<String>,
}

const TAIL: f64 = 1.2; // fin au plus tard après le début du dernier mot
const MIN_SHOW: f64 = 0.5; // durée d'affichage minimale

fn f(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

/// Entrées des sous-titres d'un edit.json (vides ou sans texte : ignorés).
pub fn cues(edit: &Value) -> Vec<Cue> {
    let caps = util::arr(edit, "captions");
    let mut out: Vec<Cue> = vec![];
    for (i, c) in caps.iter().enumerate() {
        let text = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
        let lines: Vec<String> = [text("top"), text("bottom")].into_iter().filter(|l| !l.is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        let start = f(c, "start");
        let mut end = f(c, "end");
        if let Some(last) = util::arr(c, "times").last().and_then(Value::as_f64) {
            end = end.min(last + TAIL);
        }
        if let Some(next) = caps.get(i + 1) {
            end = end.min(f(next, "start"));
        }
        end = end.max(start + MIN_SHOW);
        out.push(Cue { start, end, lines });
    }
    // Pas de chevauchement (un minimum d'affichage a pu déborder sur le suivant).
    for k in 1..out.len() {
        if out[k - 1].end > out[k].start {
            out[k - 1].end = out[k].start.max(out[k - 1].start + 0.04);
        }
    }
    out
}

/// Temps « HH:MM:SS,mmm » (SRT) ou « HH:MM:SS.mmm » (VTT).
pub fn stamp(t: f64, sep: char) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as i64;
    format!("{:02}:{:02}:{:02}{sep}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn srt(cues: &[Cue]) -> String {
    let mut s = String::new();
    for (i, c) in cues.iter().enumerate() {
        let lines: Vec<String> = c.lines.iter().map(|l| l.replace("-->", "→")).collect();
        s.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, stamp(c.start, ','), stamp(c.end, ','), lines.join("\n")));
    }
    s
}

pub fn vtt(cues: &[Cue]) -> String {
    let mut s = String::from("WEBVTT\n\n");
    for c in cues {
        // « --> » ne peut pas apparaître dans le texte d'une entrée WebVTT
        let lines: Vec<String> = c.lines.iter().map(|l| l.replace("-->", "→").replace('&', "&amp;").replace('<', "&lt;")).collect();
        s.push_str(&format!("{} --> {}\n{}\n\n", stamp(c.start, '.'), stamp(c.end, '.'), lines.join("\n")));
    }
    s
}

/// Fichiers écrits pour le montage en cours (out/montage.srt, out/montage.vtt) ; aucun si le
/// montage n'a pas de sous-titres (les anciens sont alors effacés).
pub fn write_all() -> Result<Vec<PathBuf>, String> {
    let edit = util::read_json(paths::data().join("edit.json")).ok_or("src/data/edit.json illisible")?;
    let cues = cues(&edit);
    let out = paths::out();
    util::mkdirs(&out);
    let files = [(out.join("montage.srt"), srt(&cues)), (out.join("montage.vtt"), vtt(&cues))];
    let mut done = vec![];
    for (path, text) in files {
        if cues.is_empty() {
            let _ = fs::remove_file(&path);
            continue;
        }
        fs::write(&path, text).map_err(|e| format!("{} : {e}", path.display()))?;
        done.push(path);
    }
    Ok(done)
}

pub fn main(_args: &[String]) -> i32 {
    match write_all() {
        Ok(files) if files.is_empty() => {
            println!("Pas de sous-titres dans ce montage : aucun fichier.");
            0
        }
        Ok(files) => {
            for p in files {
                println!("Sous-titres : {}", p.display());
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn edit() -> Value {
        json!({"captions": [
            {"top": "Bonjour à", "bottom": "tous", "words": ["Bonjour", "à", "tous"], "times": [0.2, 0.6, 0.8], "start": 0.2, "end": 1.4},
            {"top": "", "bottom": "", "words": ["", ""], "times": [1.4, 1.7], "start": 1.4, "end": 2.0},
            {"top": "Une phrase", "bottom": "--> fin", "words": ["Une", "phrase", "-->", "fin"], "times": [2.0, 2.3, 2.6, 2.9], "start": 2.0, "end": 9.0},
        ]})
    }

    #[test]
    fn cues_skip_empty_and_cap_silence() {
        let c = cues(&edit());
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].lines, vec!["Bonjour à", "tous"]);
        assert!((c[0].end - 1.4).abs() < 1e-9);
        // Dernier sous-titre : fin 1,2 s après le début du dernier mot, pas à 9 s.
        assert!((c[1].end - 4.1).abs() < 1e-9);
    }

    #[test]
    fn srt_and_vtt_text() {
        let c = cues(&edit());
        let s = srt(&c);
        assert!(s.starts_with("1\n00:00:00,200 --> 00:00:01,400\nBonjour à\ntous\n\n2\n"));
        let v = vtt(&c);
        assert!(v.starts_with("WEBVTT\n\n00:00:00.200 --> 00:00:01.400\n"));
        assert!(v.contains("→ fin") && !v.contains("--> fin"));
        assert_eq!(stamp(3725.5, ','), "01:02:05,500");
    }
}
