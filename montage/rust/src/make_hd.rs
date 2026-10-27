//! Source 4K pour le rendu final (le double de la version de travail : 2160x3840 en 9:16,
//! 3840x2160 en 16:9, 2880x2160 en 4:3…), limitée aux images gardées au montage.
//!
//! Au lieu d'encoder tout le rush en 4K, on ne garde que les plages « hdRanges » de
//! src/data/edit.json (clips + marge, calculées par build_edit), mises bout à bout dans
//! public/rushes/rush_2160.mp4. Les images sont numérotées comme dans la version d'aperçu
//! (30 i/s constants), donc la correspondance avec trimBeforeHd est exacte.
//!
//! Lit le rush d'origine (work/rush.txt : la vidéo envoyée, ou l'assemblage de join_rushes), pas
//! la version de travail : un rush 4K garde ses images (même taille : rien n'est recalculé ; plus
//! grand, 5K, 6K… : réduit), un rush plus petit est agrandi. Même géométrie que la version de
//! travail (image entière mise à la taille du montage, `scale`). Les images gardées sont choisies
//! avant la mise à l'échelle (mêmes images, seules celles-là passent par `scale`). H.264 presque
//! sans perte (crf 16, une image clé toutes les 15 pour les sauts du rendu), par la carte graphique
//! si elle marche (gpu.rs ; échec sur la carte : refait par le processeur).
//! Ne refait rien si les plages n'ont pas changé depuis le dernier encodage (work/hd_key.txt).

use crate::gpu::{self, Encoder, Rate};
use crate::moments_lib::display_size;
use crate::paths;
use crate::progress;
use crate::pyjson;
use crate::util::{self, read_json};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Fichier de la source 4K dans public/rushes/ (lu par le rendu final : render_lib::Montage::rush).
pub const FILE: &str = "rush_2160.mp4";
/// Source 4K des versions précédentes (VP9), effacée une fois la nouvelle faite.
pub const OLD_FILE: &str = "rush_2160.webm";
/// Qualité de la source 4K (crf de libx264 : presque sans perte).
const CRF: u32 = 16;

/// `str()` d'un nombre JSON comme Python (12, 12.5).
fn n(v: &Value) -> String {
    pyjson::dumps(v)
}

/// `b - 1` ou `b - a` sur des nombres JSON : entier si les deux le sont.
fn minus(b: &Value, a: &Value) -> Value {
    match (b.as_i64(), a.as_i64()) {
        (Some(x), Some(y)) => Value::from(x - y),
        _ => util::num(b.as_f64().unwrap_or(0.0) - a.as_f64().unwrap_or(0.0)),
    }
}

/// Filtre de la source 4K : cadence du montage (30 i/s), images gardées (`select`), puis mise à
/// la taille 4K et numérotation continue. `tail` : fin propre à l'encodeur (gpu::Encoder::filter_tail).
pub fn video_filter(select: &str, width: i64, height: i64, tail: &str) -> String {
    format!("fps=30,select='{select}',scale={width}:{height},setsar=1,settb=1/30,setpts=N{tail}")
}

/// Commande ffmpeg de la source 4K avec l'encodeur `enc`.
pub fn command(enc: &Encoder, rush: &Path, select: &str, size: (i64, i64), out: &Path) -> Vec<String> {
    let s = |x: &str| x.to_string();
    let mut cmd = vec![paths::display(paths::ffmpeg()), s("-v"), s("error"), s("-y")];
    cmd.extend(enc.input_args());
    cmd.extend([s("-i"), paths::display(rush), s("-map"), s("0:v:0"), s("-an"), s("-vf"), video_filter(select, size.0, size.1, enc.filter_tail())]);
    cmd.extend(enc.video_args(Rate::Crf(CRF), "veryfast"));
    // 30 i/s constants : sans « -r », le MP4 prendrait 25 i/s et sauterait des images.
    cmd.extend([s("-r"), s("30"), s("-g"), s("15"), paths::display(out)]);
    cmd
}

pub fn main(_args: &[String]) -> i32 {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn run() -> Result<i32, String> {
    let root = paths::root();
    let out_video = paths::rushes().join(FILE);
    let key_file = paths::work().join("hd_key.txt");

    let edit = read_json(paths::data().join("edit.json")).ok_or("src/data/edit.json illisible")?;
    let rush = util::read_text(paths::work().join("rush.txt")).ok_or("work/rush.txt illisible")?;

    // Format du rush : int(edit.get("width") or 1080).
    let dim = |k: &str, d: i64| -> i64 {
        match edit.get(k) {
            Some(v) if util::truthy(Some(v)) => v.as_i64().or_else(|| v.as_f64().map(|x| x as i64)).or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(d),
            _ => d,
        }
    };
    let (width, height) = (dim("width", 1080), dim("height", 1920));
    let ranges: Vec<(Value, Value)> = util::arr(&edit, "hdRanges") // calculées par build_edit
        .iter()
        .map(|r| (r.get(0).cloned().unwrap_or(Value::from(0)), r.get(1).cloned().unwrap_or(Value::from(0))))
        .collect();
    if edit.get("hdRanges").is_none() {
        return Err("src/data/edit.json : hdRanges absent".into());
    }
    let mut offset = Value::from(0);
    for (a, b) in &ranges {
        offset = match (offset.as_i64(), minus(b, a)) {
            (Some(o), d) if d.is_i64() => Value::from(o + d.as_i64().unwrap()),
            (_, d) => util::num(offset.as_f64().unwrap_or(0.0) + d.as_f64().unwrap_or(0.0)),
        };
    }
    let offset_f = offset.as_f64().unwrap_or(0.0);

    let key = format!("{rush}|{}", ranges.iter().map(|(a, b)| format!("{}-{}", n(a), n(b))).collect::<Vec<_>>().join(","));
    let previous = fs::read_to_string(&key_file).unwrap_or_default();
    if previous == key && out_video.exists() {
        println!("Source 4K déjà à jour");
        return Ok(0);
    }

    let select = ranges
        .iter()
        .map(|(a, b)| format!("between(n\\,{}\\,{})", n(a), n(&minus(b, &Value::from(1)))))
        .collect::<Vec<_>>()
        .join("+");
    let size = (2 * width, 2 * height);
    let rush_path = root.join(&rush);
    let origin = match display_size(&rush_path) {
        Some((w, h)) if (w, h) == size => format!("rush {w}x{h} : images d'origine"),
        Some((w, h)) if w >= size.0 && h >= size.1 => format!("rush {w}x{h} : images d'origine réduites"),
        Some((w, h)) => format!("rush {w}x{h} agrandi"),
        None => "taille du rush inconnue".into(),
    };
    progress::say(&format!("Encodage 4K {}x{} ({origin}) de {} images ({:.1} s)…", size.0, size.1, n(&offset), offset_f / 30.0));
    let part = paths::rushes().join("rush_2160.part.mp4"); // renommé une fois complet (« Annuler » possible)
    let code = gpu::encode(|enc| progress::ffmpeg("Source 4K", &command(enc, &rush_path, &select, size, &part), Some(offset_f / 30.0), 0.0, None));
    if code != 0 {
        let _ = fs::remove_file(&part);
        return Err(format!("Échec de l'encodage de la source 4K (ffmpeg, code {code})"));
    }
    fs::rename(&part, &out_video).map_err(|e| e.to_string())?;
    fs::write(&key_file, &key).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(paths::rushes().join(OLD_FILE));
    println!("Source 4K prête : {}", Path::new("public").join("rushes").join(FILE).display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_chosen_before_scaling() {
        let cmd = command(&Encoder::cpu(), Path::new("r.mov"), "between(n\\,0\\,149)", (3840, 2160), Path::new("o.mp4")).join(" ");
        assert!(cmd.contains("-vf fps=30,select='between(n\\,0\\,149)',scale=3840:2160,setsar=1,settb=1/30,setpts=N "), "{cmd}");
        assert!(cmd.ends_with("-c:v libx264 -preset veryfast -crf 16 -pix_fmt yuv420p -r 30 -g 15 o.mp4"), "{cmd}");
        assert!(!cmd.contains("hwaccel"));
        let va = command(&Encoder { name: "h264_vaapi".into() }, Path::new("r.mov"), "between(n\\,0\\,149)", (3840, 2160), Path::new("o.mp4")).join(" ");
        assert!(va.contains("-vaapi_device /dev/dri/renderD128 -hwaccel auto -i r.mov"), "{va}");
        assert!(va.contains("setpts=N,format=nv12,hwupload -c:v h264_vaapi"), "{va}");
    }
}
