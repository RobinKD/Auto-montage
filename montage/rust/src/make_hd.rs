//! Source 4K pour le rendu final (le double de la version de travail : 2160x3840 en 9:16,
//! 3840x2160 en 16:9, 2880x2160 en 4:3…), limitée aux images gardées au montage.
//!
//! Au lieu d'encoder tout le rush en 4K, on ne garde que les plages « hdRanges » de
//! src/data/edit.json (clips + marge, calculées par build_edit), mises bout à bout dans
//! public/rushes/rush_2160.webm. Les images sont numérotées comme dans la version d'aperçu
//! (30 i/s constants), donc la correspondance avec trimBeforeHd est exacte.
//! Ne refait rien si les plages n'ont pas changé depuis le dernier encodage (work/hd_key.txt).

use crate::paths;
use crate::progress;
use crate::pyjson;
use crate::util::{self, read_json};
use serde_json::Value;
use std::fs;
use std::path::Path;

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
    let out_video = paths::rushes().join("rush_2160.webm");
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
    progress::say(&format!("Encodage 4K de {} images ({:.1} s)…", n(&offset), offset_f / 30.0));
    let part = paths::rushes().join("rush_2160.part.webm"); // renommé une fois complet (« Annuler » possible)
    let cmd: Vec<String> = vec![
        paths::ffmpeg().to_string_lossy().into_owned(),
        "-v".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        root.join(&rush).to_string_lossy().into_owned(),
        "-map".into(),
        "0:v:0".into(),
        "-an".into(),
        "-vf".into(),
        format!("scale={}:{},setsar=1,fps=30,select='{select}',settb=1/30,setpts=N", 2 * width, 2 * height),
        "-c:v".into(),
        "libvpx-vp9".into(),
        "-deadline".into(),
        "good".into(),
        "-cpu-used".into(),
        "5".into(),
        "-row-mt".into(),
        "1".into(),
        "-b:v".into(),
        "0".into(),
        "-crf".into(),
        "20".into(),
        "-g".into(),
        "15".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        part.to_string_lossy().into_owned(),
    ];
    let code = progress::ffmpeg("Source 4K", &cmd, Some(offset_f / 30.0), 0.0, None);
    if code != 0 {
        return Err(format!("Échec de l'encodage de la source 4K (ffmpeg, code {code})"));
    }
    fs::rename(&part, &out_video).map_err(|e| e.to_string())?;
    fs::write(&key_file, &key).map_err(|e| e.to_string())?;
    println!("Source 4K prête : {}", Path::new("public").join("rushes").join("rush_2160.webm").display());
    Ok(0)
}
