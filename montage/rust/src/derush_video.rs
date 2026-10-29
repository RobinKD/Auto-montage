//! Dérush validé, passage à l'auto-montage : les moments gardés du dérush (src/data/edit.json,
//! écrit par build_edit en mode Dérushage) mis bout à bout en une vidéo, nouveau rush que
//! l'auto-montage prépare ensuite comme un seul moment.
//!
//! Usage : auto-montage derush_video [<nom de la vidéo>]
//! Sortie : public/rushes/<nom> (par défaut « <rush> (dérush).mp4 ») depuis le rush d'origine
//! (work/rush.txt, qualité d'origine, H.264 CRF 16, toutes les pistes son mélangées), et
//! work/derush_segments.json : un seul segment de toute la vidéo avec les mots gardés (textes
//! corrigés sur la page, temps de la vidéo), repris par `prepare --transcription` sans Whisper.
//!
//! Ce que la préparation du rush a déjà trouvé est repris au lieu d'être recalculé (work/derush_suite/,
//! installé par prepare, qui saute alors la copie 1080p, le visage et les locuteurs) :
//!   rush_1080.mp4   version de travail, encodée dans le même passage que la vidéo du dérush (rush
//!                   déjà au format de travail : la vidéo du dérush elle-même, images clés toutes
//!                   les 15 comme la version de travail) ;
//!   face.json       positions des visages des plages gardées, recalées sur la vidéo du dérush
//!                   (instant le plus proche du rush, toutes les 0,5 s), vignettes dans visages/ ;
//!   locuteurs.json  personne de chaque locuteur ; le locuteur de chaque mot (« spk ») va dans
//!                   work/derush_segments.json (mot du rush le plus recouvrant) ;
//!   cle.txt         nom de la vidéo du dérush (suite d'un autre dérush : ignorée).

use crate::moments_lib::{audio_map_of, audio_streams, display_size, work_size};
use crate::{paths, progress, util};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// Ce que la préparation du rush a trouvé, recalé sur la vidéo du dérush (lu par prepare).
pub fn suite_dir() -> PathBuf {
    paths::work().join("derush_suite")
}

/// Pas des positions du visage (detect_face).
const FACE_STEP: f64 = 0.5;

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
/// son mélangé, puis mises bout à bout. `mix` : filtre du son mélangé, sortie [son]. `work` :
/// aussi la version de travail à cette taille ([wv], [wa]) ; sinon [vout] a l'image carrée de la
/// version de travail (setsar=1 : la vidéo du dérush en tient lieu).
fn filter(ranges: &[(f64, f64)], mix: &str, work: Option<(i64, i64)>) -> String {
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
    match work {
        Some((w, h)) => g.push_str(&format!(
            "concat=n={n}:v=1:a=1[cv][ca];[cv]split=2[vout][cw];[cw]scale={w}:{h},setsar=1[wv];[ca]asplit=2[aout][wa]"
        )),
        None => g.push_str(&format!("concat=n={n}:v=1:a=1[cv][aout];[cv]setsar=1[vout]")),
    }
    g
}

/// Instant du rush d'un instant de la vidéo du dérush (plans d'edit.json : from, trimBefore, durée).
fn rush_time(clips: &[(f64, f64, f64)], t: f64) -> f64 {
    let c = clips.iter().rev().find(|c| t >= c.0).or(clips.first());
    match c {
        Some(&(from, start, dur)) => start + (t - from).clamp(0.0, dur),
        None => t,
    }
}

/// Plans du dérush en secondes : (début dans la vidéo du dérush, début dans le rush, durée).
fn clip_times(edit: &Value) -> Vec<(f64, f64, f64)> {
    let fps = edit.get("fps").and_then(Value::as_f64).filter(|f| *f > 0.0).unwrap_or(30.0);
    let mut from = 0.0;
    util::arr(edit, "clips")
        .iter()
        .filter_map(|c| {
            let a = c.get("trimBefore")?.as_f64()?;
            let d = c.get("durationInFrames")?.as_f64()?;
            if d <= 0.0 {
                return None;
            }
            let f = c.get("from").and_then(Value::as_f64).unwrap_or(from);
            from = f + d;
            Some((f / fps, a / fps, d / fps))
        })
        .collect()
}

/// Positions du visage (face.json) recalées : un instant toutes les 0,5 s de la vidéo du dérush,
/// celui du rush le plus proche.
fn face_samples(face: &[Value], clips: &[(f64, f64, f64)], total: f64) -> Vec<Value> {
    if face.is_empty() {
        return vec![];
    }
    let count = ((total + FACE_STEP) / FACE_STEP).ceil() as usize;
    (0..count)
        .map(|k| {
            let t = k as f64 * FACE_STEP;
            let j = ((rush_time(clips, t) / FACE_STEP).round().max(0.0) as usize).min(face.len() - 1);
            let mut v = face[j].clone();
            if let Value::Object(o) = &mut v {
                o.insert("t".into(), crate::util::num(t));
            }
            v
        })
        .collect()
}

/// Locuteur (« spk ») de chaque mot du dérush : celui du mot du rush qui le recouvre le plus
/// (segments.json, diarize) ; « spk » du segment : le locuteur qui y parle le plus. Rien sans
/// diarisation.
fn add_speakers(segment: &mut Value, rush_segments: &Value, clips: &[(f64, f64, f64)]) -> bool {
    let mut old: Vec<(f64, f64, i64)> = vec![];
    for s in rush_segments.as_array().map(Vec::as_slice).unwrap_or(&[]) {
        for w in util::arr(s, "words") {
            let Some(k) = w.get("spk").and_then(Value::as_i64) else { return false };
            old.push((util::fd(w, "a", 0.0), util::fd(w, "b", 0.0), k));
        }
    }
    if old.is_empty() {
        return false;
    }
    let mut dur: std::collections::BTreeMap<i64, f64> = Default::default();
    if let Some(Value::Array(words)) = segment.get_mut("words") {
        for w in words.iter_mut() {
            let (a, b) = (util::fd(w, "a", 0.0), util::fd(w, "b", 0.0));
            let ra = rush_time(clips, a);
            let rb = ra + (b - a);
            let mid = (ra + rb) / 2.0;
            let best = old
                .iter()
                .max_by(|x, y| {
                    let ox = (rb.min(x.1) - ra.max(x.0)).max(0.0) - ((x.0 + x.1) / 2.0 - mid).abs() * 1e-3;
                    let oy = (rb.min(y.1) - ra.max(y.0)).max(0.0) - ((y.0 + y.1) / 2.0 - mid).abs() * 1e-3;
                    ox.total_cmp(&oy)
                })
                .map(|x| x.2)
                .unwrap_or(0);
            if let Value::Object(o) = w {
                o.insert("spk".into(), json!(best));
            }
            *dur.entry(best).or_default() += (b - a).max(0.01);
        }
    }
    if let (Some((k, _)), Value::Object(o)) = (dur.iter().max_by(|x, y| x.1.total_cmp(y.1)), segment) {
        o.insert("spk".into(), json!(k));
    }
    true
}

/// work/derush_suite/ : positions du visage, vignettes et locuteurs de la préparation du rush.
fn carry_over(edit: &Value, total: f64, name: &str, segments: &mut Value) {
    let work = paths::work();
    let dir = suite_dir();
    let clips = clip_times(edit);
    if let Some(Value::Array(face)) = util::read_json(paths::data().join("face.json")) {
        let _ = util::write_json(dir.join("face.json"), &Value::Array(face_samples(&face, &clips, total)), None);
        if work.join("visages").is_dir() {
            let _ = util::copy_tree(&work.join("visages"), &dir.join("visages"));
        }
    }
    let rush_segments = util::read_json(work.join("segments.json")).unwrap_or(Value::Null);
    if work.join("locuteurs.json").is_file() && add_speakers(&mut segments[0], &rush_segments, &clips) {
        let _ = fs::copy(work.join("locuteurs.json"), dir.join("locuteurs.json"));
    }
    let _ = fs::write(dir.join("cle.txt"), format!("{name}\n"));
}

/// Copie (lien physique si possible) d'un fichier.
fn link_or_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = fs::remove_file(dst);
    fs::hard_link(src, dst).or_else(|_| fs::copy(src, dst).map(|_| ()))
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
    // Version de travail de la vidéo du dérush, faite dans le même passage (work/derush_suite/) :
    // à part si le rush n'est pas déjà à la taille de travail, sinon la vidéo du dérush elle-même.
    let suite = suite_dir();
    util::remove_all(&suite);
    util::mkdirs(&suite);
    let size = display_size(&rush_path);
    let work_out = size.map(|(w, h)| work_size(w, h, 1080)).filter(|ws| Some(*ws) != size);
    let graph = filter(&ranges, &map[1], work_out);
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
    // Images clés toutes les 15 comme la version de travail (version_travail) : la vidéo du
    // dérush en tient lieu quand le rush est déjà à cette taille.
    let gop: &[&str] = if work_out.is_none() { &["-g", "15"] } else { &[] };
    cmd.extend(["-map", "[vout]", "-map", "[aout]", "-c:v", "libx264", "-crf", "16", "-preset", "veryfast"].iter().map(|s| s.to_string()));
    cmd.extend(gop.iter().map(|s| s.to_string()));
    cmd.extend(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"].iter().map(|s| s.to_string()));
    cmd.push(paths::display(&part));
    let work_part = suite.join("rush_1080.part.mp4");
    if work_out.is_some() {
        cmd.extend(
            ["-map", "[wv]", "-map", "[wa]", "-c:v", "libx264", "-crf", "18", "-preset", "veryfast", "-g", "15", "-c:a", "aac", "-b:a", "192k"]
                .iter()
                .map(|s| s.to_string()),
        );
        cmd.push(paths::display(&work_part));
    }
    progress::say(&format!("Vidéo du dérush : {} moments, {} s", ranges.len(), util::fmt_g((total * 10.0).round() / 10.0, 12)));
    let code = progress::ffmpeg("Vidéo du dérush", &cmd, Some(total), 0.0, None);
    let _ = fs::remove_file(&script);
    if code != 0 {
        let _ = fs::remove_file(&part);
        util::remove_all(&suite);
        return code;
    }
    if let Err(e) = fs::rename(&part, &dest) {
        eprintln!("public/rushes/{name} : {e}");
        return 1;
    }
    let work_video = suite.join("rush_1080.mp4");
    let ready = if work_out.is_some() { fs::rename(&work_part, &work_video) } else { link_or_copy(&dest, &work_video) };
    if let Err(e) = ready {
        eprintln!("Version de travail du dérush : {e} (elle sera refaite)");
    }
    let mut segments = transcription(&edit, total);
    carry_over(&edit, total, &name, &mut segments);
    if let Err(e) = util::write_json(work.join("derush_segments.json"), &segments, None) {
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
        let g = filter(&r, "[0:a:0]anull[son]", None);
        assert!(g.contains("trim=start_frame=30:end_frame=90"));
        assert!(g.contains("atrim=start=5:end=5.5"));
        assert!(g.ends_with("[cv0][ca0][cv1][ca1]concat=n=2:v=1:a=1[cv][aout];[cv]setsar=1[vout]"));
        let g = filter(&r, "[0:a:0]anull[son]", Some((1920, 1080)));
        assert!(g.ends_with("[cv]split=2[vout][cw];[cw]scale=1920:1080,setsar=1[wv];[ca]asplit=2[aout][wa]"));
        let t = transcription(&edit, 2.5);
        assert_eq!(t[0]["text"], "Bonjour tous");
        assert_eq!(t[0]["end"], 2.5);
    }

    #[test]
    fn carried_over_to_the_derush_timeline() {
        // plans : rush 1-3 s puis 5-5,5 s → dérush 0-2 s puis 2-2,5 s
        let edit = json!({"fps": 30, "clips": [{"trimBefore": 30, "durationInFrames": 60, "from": 0}, {"trimBefore": 150, "durationInFrames": 15, "from": 60}]});
        let clips = clip_times(&edit);
        assert_eq!(rush_time(&clips, 0.5), 1.5);
        assert_eq!(rush_time(&clips, 2.25), 5.25);
        let face: Vec<Value> = (0..12).map(|k| json!({"t": k as f64 * 0.5, "x": k})).collect();
        let f = face_samples(&face, &clips, 2.5);
        assert_eq!(f.len(), 6);
        assert_eq!((f[1]["t"].clone(), f[1]["x"].clone()), (json!(0.5), json!(3))); // rush 1,5 s
        assert_eq!(f[5]["x"], json!(11)); // dérush 2,5 s → rush 5,5 s
        let rush = json!([{"words": [{"w": "a", "a": 1.0, "b": 1.4, "spk": 0}, {"w": "b", "a": 5.0, "b": 5.4, "spk": 1}]}]);
        let mut seg = json!({"words": [{"w": "a", "a": 0.0, "b": 0.4}, {"w": "b", "a": 2.0, "b": 2.4}, {"w": "b2", "a": 2.05, "b": 2.45}]});
        assert!(add_speakers(&mut seg, &rush, &clips));
        let spk: Vec<Value> = util::arr(&seg, "words").iter().map(|w| w["spk"].clone()).collect();
        assert_eq!(spk, vec![json!(0), json!(1), json!(1)]);
        assert_eq!(seg["spk"], json!(1));
        assert!(!add_speakers(&mut seg, &json!([{"words": [{"w": "a", "a": 1.0, "b": 1.4}]}]), &clips));
    }
}
