//! Plusieurs vidéos pour un même rush (caméra coupée en cours de tournage, batterie, carte
//! pleine…) : assemblées dans l'ordre en une seule vidéo, que la préparation traite ensuite
//! comme n'importe quel rush.
//!
//! Usage : auto-montage join_rushes <vidéo 1> <vidéo 2> [<vidéo 3>…]   assemble
//!         auto-montage join_rushes --name <vidéo 1> <vidéo 2> […]     nom du rush assemblé
//! Les vidéos sont dans public/rushes/ ; le rush assemblé aussi (« <vidéo 1> (assemblage de N).mov »).
//! Vidéos de mêmes réglages (codec, taille, cadence, son) : mises bout à bout sans réencodage
//! (quelques secondes, qualité d'origine). Réglages différents : réencodées aux réglages de la
//! première (taille, cadence ; H.264 haute qualité), plus long. Déjà assemblées à l'identique
//! (mêmes vidéos, même ordre) : rien n'est refait.

use crate::moments_lib::audio_streams;
use crate::util::{fmt_g, round_int};
use crate::{paths, procs, progress, pyjson};
use regex::Regex;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const DOC: &str = "Plusieurs vidéos pour un même rush : assemblées dans l'ordre en une seule vidéo.

Usage : auto-montage join_rushes <vidéo 1> <vidéo 2> [<vidéo 3>…]   assemble
        auto-montage join_rushes --name <vidéo 1> <vidéo 2> […]     nom du rush assemblé
Les vidéos sont dans public/rushes/ ; le rush assemblé aussi (« <vidéo 1> (assemblage de N).mov »).";

/// Nom du rush assemblé à partir de plusieurs vidéos (« essai (assemblage de 2).mov »).
pub fn joined_name(names: &[String]) -> String {
    let first = std::path::Path::new(&names[0]);
    let stem: String = first.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default().chars().take(150).collect();
    format!("{stem} (assemblage de {}).mov", names.len())
}

static PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^()]*\)").unwrap());
static VIDEO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Stream #\d+:\d+.*?: Video: (.*)").unwrap());
static AUDIO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Stream #\d+:\d+.*?: Audio: (.*)").unwrap());
static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)x(\d+)").unwrap());
static FPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([\d.]+) fps").unwrap());
static TBR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([\d.]+) tbr").unwrap());
static ROT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"rotation of (-?[\d.]+) degrees").unwrap());

/// Retire les parenthèses (« yuv420p(tv, bt709) ») : restent les champs séparés par des virgules.
fn flat(text: &str) -> Vec<String> {
    let mut text = text.to_string();
    loop {
        let new = PAREN.replace_all(&text, "").into_owned();
        if new == text {
            return text.split(',').map(|f| f.trim().to_string()).collect();
        }
        text = new;
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Info {
    duration: f64,
    vcodec: String,
    pix: String,
    width: i64,
    height: i64,
    fps: f64,
    rotation: i64,
    tracks: Vec<usize>,
    acodec: Option<String>,
    rate: Option<String>,
    layout: Option<String>,
}

fn first_word(s: &str) -> String {
    s.split_whitespace().next().unwrap_or("").to_string()
}

/// Réglages d'une vidéo, lus dans la sortie de « ffmpeg -i ».
fn probe(path: &Path) -> Result<Info, String> {
    let out = procs::ffmpeg_info(path);
    let duration = progress::duration_of(&out);
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(video) = VIDEO.captures(&out).map(|c| c[1].to_string()) else {
        return Err(format!("Pas d'image dans {name} : est-ce bien une vidéo ?"));
    };
    let fields = flat(&video);
    let size = fields.iter().find_map(|f| SIZE.captures(f).map(|c| (c[1].parse::<i64>().unwrap_or(0), c[2].parse::<i64>().unwrap_or(0))));
    let fps = FPS.captures(&video).or_else(|| TBR.captures(&video)).and_then(|c| c[1].parse::<f64>().ok());
    let rotation = ROT.captures(&out).map(|c| round_int(c[1].parse::<f64>().unwrap_or(0.0)).rem_euclid(360)).unwrap_or(0);
    // Pistes son lisibles (plusieurs : un micro par personne), mélangées par prepare.
    let tracks: Vec<usize> = audio_streams(path).into_iter().map(|(i, _)| i).collect();
    let (mut acodec, mut rate, mut layout) = (None, None, None);
    if let Some(audio) = AUDIO.captures(&out) {
        if !tracks.is_empty() {
            let a = flat(&audio[1]);
            acodec = Some(first_word(&a[0]));
            rate = Some(a.get(1).cloned().unwrap_or_default());
            layout = Some(a.get(2).cloned().unwrap_or_default());
        }
    }
    Ok(Info {
        duration,
        vcodec: first_word(&fields[0]),
        pix: fields.get(1).cloned().unwrap_or_default(),
        width: size.map(|s| s.0).unwrap_or(0),
        height: size.map(|s| s.1).unwrap_or(0),
        fps: fps.unwrap_or(30.0),
        rotation,
        tracks,
        acodec,
        rate,
        layout,
    })
}

fn same_settings(infos: &[Info]) -> bool {
    let key = |i: &Info| (i.vcodec.clone(), i.pix.clone(), i.width, i.height, i.fps, i.rotation, i.acodec.clone(), i.rate.clone(), i.layout.clone(), i.tracks.clone());
    infos.iter().all(|i| i.acodec.as_deref().is_some_and(|a| !a.is_empty())) && infos[1..].iter().all(|i| key(i) == key(&infos[0]))
}

fn signature(paths: &[PathBuf]) -> Value {
    Value::Array(
        paths
            .iter()
            .map(|p| {
                let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let size = fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                json!([name, size, crate::util::mtime(p).unwrap_or(0.0) as i64])
            })
            .collect(),
    )
}

fn basename(n: &str) -> String {
    Path::new(n).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Assemble les vidéos ; renvoie le chemin du rush assemblé.
pub fn join(names: &[String]) -> Result<PathBuf, String> {
    let rushes = paths::rushes();
    let paths: Vec<PathBuf> = names.iter().map(|n| rushes.join(basename(n))).collect();
    for p in &paths {
        if !p.is_file() {
            return Err(format!("Vidéo introuvable : {}", paths::display(p)));
        }
    }
    let out = rushes.join(joined_name(names));
    let out_name = joined_name(names);
    let meta = rushes.join(format!(".{out_name}.sources.json"));
    if out.is_file() && crate::util::read_json(&meta).as_ref() == Some(&signature(&paths)) {
        println!("Déjà assemblé : {out_name}");
        return Ok(out);
    }
    let infos = paths.iter().map(|p| probe(p)).collect::<Result<Vec<_>, _>>()?;
    let total: f64 = infos.iter().map(|i| i.duration).sum();
    let part = rushes.join(format!("{}.part.mov", &out_name[..out_name.len() - 4]));
    let ffmpeg = paths::display(paths::ffmpeg());
    let code;
    if same_settings(&infos) {
        progress::say(&format!("Assemblage de {} vidéos aux mêmes réglages, sans réencodage", paths.len()));
        let mut text = String::new();
        for p in &paths {
            let s = paths::display(p);
            if s.contains('\n') || s.contains('\r') {
                // une ligne en plus ajouterait un autre fichier à la liste
                return Err(format!("Nom de fichier invalide (retour à la ligne) : {s:?}"));
            }
            text += &format!("file '{}'\n", s.replace('\'', "'\\''"));
        }
        let listing = std::env::temp_dir().join(format!("am-assemblage-{}-{}.txt", std::process::id(), crate::util::now_secs() as u64));
        fs::write(&listing, text).map_err(|e| e.to_string())?;
        let mut cmd: Vec<String> = vec![ffmpeg, "-v".into(), "error".into(), "-y".into(), "-f".into(), "concat".into(), "-safe".into(), "0".into(), "-i".into(), paths::display(&listing), "-map".into(), "0:v:0".into()];
        for i in &infos[0].tracks {
            cmd.extend(["-map".into(), format!("0:a:{i}")]);
        }
        cmd.extend(["-c".into(), "copy".into(), "-movflags".into(), "+faststart".into(), paths::display(&part)]);
        code = progress::ffmpeg("Assemblage des vidéos", &cmd, Some(total), 0.0, None);
        let _ = fs::remove_file(&listing);
    } else {
        let first = &infos[0];
        // Taille affichée de la première (vidéo de téléphone tournée : largeur et hauteur inversées).
        let (w, h) = if first.rotation == 90 || first.rotation == 270 { (first.height, first.width) } else { (first.width, first.height) };
        let (w, h) = (w - w % 2, h - h % 2);
        let fps = if (1.0..=120.0).contains(&first.fps) { first.fps } else { 30.0 };
        let fps_g = fmt_g(fps, 6);
        progress::say(&format!("Assemblage de {} vidéos aux réglages différents : réencodage en {w}x{h}, {fps_g} images/s (plus long)", paths.len()));
        for (p, i) in paths.iter().zip(&infos) {
            progress::say(&format!(
                "  {} : {}x{}, {} images/s, {}, son {}{}",
                basename(&paths::display(p)),
                i.width,
                i.height,
                fmt_g(i.fps, 6),
                i.vcodec,
                i.acodec.as_deref().filter(|a| !a.is_empty()).unwrap_or("absent"),
                if i.tracks.len() > 1 { format!(" ({} pistes)", i.tracks.len()) } else { String::new() }
            ));
        }
        let mut cmd: Vec<String> = vec![ffmpeg, "-v".into(), "error".into(), "-y".into()];
        for p in &paths {
            cmd.extend(["-i".into(), paths::display(p)]);
        }
        let n = paths.len();
        let has_sound = |i: &Info| i.acodec.as_deref().is_some_and(|a| !a.is_empty());
        for i in &infos {
            if !has_sound(i) {
                // vidéo sans son : silence de même durée
                cmd.extend(["-f".into(), "lavfi".into(), "-t".into(), format!("{:.3}", i.duration), "-i".into(), "anullsrc=r=48000:cl=stereo".into()]);
            }
        }
        let mut graph = vec![];
        let mut silent = n;
        for (k, i) in infos.iter().enumerate() {
            graph.push(format!("[{k}:v:0]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps_g},format=yuv420p[v{k}]"));
            let fmt = "aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo";
            if !has_sound(i) {
                graph.push(format!("[{silent}:a:0]{fmt}[a{k}]"));
                silent += 1;
            } else if i.tracks.len() == 1 {
                graph.push(format!("[{k}:a:{}]{fmt}[a{k}]", i.tracks[0]));
            } else {
                // plusieurs pistes son : mélangées, comme prepare (moments_lib::audio_map)
                let mut g: String = i.tracks.iter().map(|t| format!("[{k}:a:{t}]{fmt}[a{k}_{t}];")).collect();
                g += &i.tracks.iter().map(|t| format!("[a{k}_{t}]")).collect::<String>();
                g += &format!("amix=inputs={}:duration=longest:normalize=0,alimiter=limit=0.97:level=0[a{k}]", i.tracks.len());
                graph.push(g);
            }
        }
        graph.push((0..n).map(|k| format!("[v{k}][a{k}]")).collect::<String>() + &format!("concat=n={n}:v=1:a=1[v][a]"));
        cmd.extend(
            [
                "-filter_complex",
                &graph.join(";"),
                "-map",
                "[v]",
                "-map",
                "[a]",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "16",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "256k",
                "-movflags",
                "+faststart",
                &paths::display(&part),
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        code = progress::ffmpeg("Assemblage des vidéos (réencodage)", &cmd, Some(total), 0.0, None);
    }
    if code != 0 {
        let _ = fs::remove_file(&part);
        return Err("L'assemblage des vidéos a échoué (voir les messages de ffmpeg ci-dessus).".into());
    }
    fs::rename(&part, &out).map_err(|e| e.to_string())?;
    let _ = fs::write(&meta, pyjson::dumps(&signature(&paths)));
    progress::say(&format!("Rush assemblé : {out_name} ({total:.0} s)"));
    Ok(out)
}

pub fn main(args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("--name") && args.len() >= 3 {
        println!("{}", joined_name(&args[1..]));
        0
    } else if args.len() >= 2 && !args[0].starts_with('-') {
        match join(args) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        }
    } else {
        eprintln!("{DOC}");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_fields() {
        assert_eq!(flat("h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 30 fps"), vec!["h264", "yuv420p", "1920x1080 [SAR 1:1 DAR 16:9]", "30 fps"]);
        assert_eq!(joined_name(&["a/essai.mov".into(), "b.mp4".into()]), "essai (assemblage de 2).mov");
    }
}
