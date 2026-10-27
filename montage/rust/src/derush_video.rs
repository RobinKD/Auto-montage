//! Dérush validé, passage à l'auto-montage : les moments gardés du dérush (src/data/edit.json,
//! écrit par build_edit en mode Dérushage) mis bout à bout en une vidéo, nouveau rush que
//! l'auto-montage prépare ensuite comme un seul moment.
//!
//! Usage : auto-montage derush_video [<nom de la vidéo>]
//! Sortie : public/rushes/<nom> (par défaut « <rush> (dérush).mp4 ») depuis le rush d'origine
//! (work/rush.txt, qualité d'origine, H.264 CRF 16, toutes les pistes son mélangées), et
//! work/derush_segments.json : un seul segment de toute la vidéo avec les mots gardés (textes
//! corrigés sur la page, temps de la vidéo), repris par `prepare --transcription` sans Whisper.

use crate::moments_lib::{audio_map_of, audio_streams};
use crate::{paths, progress, util};
use serde_json::{json, Value};
use std::fs;

/// Nom de la vidéo du dérush d'un rush (« essai.mov » → « essai (dérush).mp4 », sans doubler
/// le suffixe pour le dérush d'un dérush).
pub fn output_name(rush: &str) -> String {
    let stem = std::path::Path::new(rush.trim()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = stem.trim_end_matches(" (dérush)");
    let stem: String = stem.chars().take(150).collect();
    format!("{stem} (dérush).mp4")
}

/// Plages gardées du rush (secondes) d'après les plans d'edit.json.
fn ranges(edit: &Value) -> Vec<(f64, f64)> {
    let fps = edit.get("fps").and_then(Value::as_f64).filter(|f| *f > 0.0).unwrap_or(30.0);
    util::arr(edit, "clips")
        .iter()
        .filter_map(|c| {
            let a = c.get("trimBefore")?.as_f64()?;
            let d = c.get("durationInFrames")?.as_f64()?;
            (d > 0.0).then_some((a / fps, (a + d) / fps))
        })
        .collect()
}

/// Graphe ffmpeg : chaque plage coupée dans l'image (30 images/s, comme le montage) et dans le
/// son mélangé, puis mises bout à bout. `mix` : filtre du son mélangé, sortie [son].
fn filter(ranges: &[(f64, f64)], mix: &str) -> String {
    let n = ranges.len();
    let mut g = format!("{mix};[son]asplit={n}");
    for i in 0..n {
        g.push_str(&format!("[a{i}]"));
    }
    g.push_str(&format!(";[0:v]fps=30,format=yuv420p,split={n}"));
    for i in 0..n {
        g.push_str(&format!("[v{i}]"));
    }
    for (i, (a, b)) in ranges.iter().enumerate() {
        let (fa, fb) = ((a * 30.0).round() as i64, (b * 30.0).round() as i64);
        g.push_str(&format!(
            ";[v{i}]trim=start_frame={fa}:end_frame={fb},setpts=PTS-STARTPTS[cv{i}];[a{i}]atrim=start={}:end={},asetpts=PTS-STARTPTS[ca{i}]",
            util::fmt_g(fa as f64 / 30.0, 12),
            util::fmt_g(fb as f64 / 30.0, 12)
        ));
    }
    g.push(';');
    for i in 0..n {
        g.push_str(&format!("[cv{i}][ca{i}]"));
    }
    g.push_str(&format!("concat=n={n}:v=1:a=1[vout][aout]"));
    g
}

/// Transcription de la vidéo du dérush : un seul segment, mots gardés (derushWords d'edit.json).
fn transcription(edit: &Value, total: f64) -> Value {
    let words: Vec<Value> = util::arr(edit, "derushWords").to_vec();
    let text: Vec<&str> = words.iter().filter_map(|w| w.get("w").and_then(Value::as_str)).collect();
    json!([{"id": 0, "start": 0.0, "end": (total * 1000.0).round() / 1000.0, "text": text.join(" "), "words": words}])
}

pub fn main(args: &[String]) -> i32 {
    let root = paths::root();
    let work = paths::work();
    let rush = util::read_text(work.join("rush.txt")).unwrap_or_default().trim().to_string();
    if rush.is_empty() || !root.join(&rush).is_file() {
        eprintln!("Rush introuvable (work/rush.txt) : préparez d'abord un rush.");
        return 1;
    }
    let Some(edit) = util::read_json(root.join("src").join("data").join("edit.json")) else {
        eprintln!("src/data/edit.json illisible : lancez d'abord build_edit.");
        return 1;
    };
    if edit.get("derushWords").is_none() {
        eprintln!("Ce montage n'est pas un dérush (src/data/edit.json écrit hors du mode Dérushage).");
        return 1;
    }
    let ranges = ranges(&edit);
    if ranges.is_empty() {
        eprintln!("Aucun moment gardé : rien à mettre dans la vidéo du dérush.");
        return 1;
    }
    let name = args.first().cloned().unwrap_or_else(|| output_name(&rush));
    let rush_path = root.join(&rush);
    let streams = audio_streams(&rush_path);
    let Some(map) = audio_map_of(&streams, "son") else {
        eprintln!("Le rush n'a pas de son lisible.");
        return 1;
    };
    let total: f64 = ranges.iter().map(|(a, b)| ((b * 30.0).round() - (a * 30.0).round()) / 30.0).sum();
    let graph = filter(&ranges, &map[1]);
    util::mkdirs(paths::rushes());
    let dest = paths::rushes().join(&name);
    let part = paths::rushes().join(format!("{name}.part.mp4"));
    // Graphe long (beaucoup de moments) : passé dans un fichier, la ligne de commande a une
    // longueur limitée (Windows).
    let script = work.join("derush_filtre.txt");
    let mut cmd: Vec<String> = vec![paths::display(paths::ffmpeg()), "-v".into(), "error".into(), "-y".into(), "-i".into(), paths::display(&rush_path)];
    if graph.len() > 6000 {
        if let Err(e) = fs::write(&script, &graph) {
            eprintln!("work/derush_filtre.txt : {e}");
            return 1;
        }
        cmd.extend(["-filter_complex_script".into(), paths::display(&script)]);
    } else {
        cmd.extend(["-filter_complex".into(), graph]);
    }
    cmd.extend(
        ["-map", "[vout]", "-map", "[aout]", "-c:v", "libx264", "-crf", "16", "-preset", "veryfast", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]
            .iter()
            .map(|s| s.to_string()),
    );
    cmd.push(paths::display(&part));
    progress::say(&format!("Vidéo du dérush : {} moments, {} s", ranges.len(), util::fmt_g((total * 10.0).round() / 10.0, 12)));
    let code = progress::ffmpeg("Vidéo du dérush", &cmd, Some(total), 0.0, None);
    let _ = fs::remove_file(&script);
    if code != 0 {
        let _ = fs::remove_file(&part);
        return code;
    }
    if let Err(e) = fs::rename(&part, &dest) {
        eprintln!("public/rushes/{name} : {e}");
        return 1;
    }
    if let Err(e) = util::write_json(work.join("derush_segments.json"), &transcription(&edit, total), None) {
        eprintln!("work/derush_segments.json : {e}");
        return 1;
    }
    println!("public/rushes/{name} ({} mots transcrits)", util::arr(&edit, "derushWords").len());
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(output_name("public/rushes/essai.mov"), "essai (dérush).mp4");
        assert_eq!(output_name("public/rushes/essai (dérush).mp4"), "essai (dérush).mp4");
    }

    #[test]
    fn graph_and_words() {
        let edit = json!({"fps": 30, "clips": [{"trimBefore": 30, "durationInFrames": 60}, {"trimBefore": 150, "durationInFrames": 15}],
                          "derushWords": [{"w": "Bonjour", "a": 0.1, "b": 0.5}, {"w": "tous", "a": 2.1, "b": 2.4}]});
        let r = ranges(&edit);
        assert_eq!(r, vec![(1.0, 3.0), (5.0, 5.5)]);
        let g = filter(&r, "[0:a:0]anull[son]");
        assert!(g.contains("trim=start_frame=30:end_frame=90"));
        assert!(g.contains("atrim=start=5:end=5.5"));
        assert!(g.ends_with("[cv0][ca0][cv1][ca1]concat=n=2:v=1:a=1[vout][aout]"));
        let t = transcription(&edit, 2.5);
        assert_eq!(t[0]["text"], "Bonjour tous");
        assert_eq!(t[0]["end"], 2.5);
    }
}
