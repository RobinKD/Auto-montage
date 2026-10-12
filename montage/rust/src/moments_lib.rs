//! Plans sans parole : les intervalles entre les segments de parole du rush, et ce que partagent
//! make_moments (page) et build_edit (montage) pour numéroter les plans de la même façon :
//! id = GAP_BASE + rang, temps en secondes dans le rush. Aussi : formats d'image, langue, mode,
//! pistes son, découpage (scissions et fusions), effets fournis par défaut, voix modifiées.

use crate::difflib::SequenceMatcher;
use crate::paths;
use crate::procs;
use crate::util::{self, read_json, round_int, round_to};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const GAP_BASE: i64 = 1000;
pub const MIN_GAP: f64 = 0.5; // plus court, ce n'est qu'une respiration entre deux phrases

/// Durée du son 16 kHz du rush (work/rush_16k.wav).
pub fn rush_duration(work_dir: &Path) -> f64 {
    hound::WavReader::open(work_dir.join("rush_16k.wav"))
        .map(|r| r.duration() as f64 / r.spec().sample_rate as f64)
        .unwrap_or(0.0)
}

// Frises de la page des moments (« façon CapCut ») : une image du rush toutes les STRIP_STEP s, en
// planches de STRIP_TILE x STRIP_TILE images (work/moments/frames/), et la forme d'onde de la voix.
pub const STRIP_STEP: f64 = 0.5;
pub const STRIP_TILE: i64 = 10;
pub const WAVE_RATE: u32 = 20; // crêtes par seconde

/// Planches d'images du rush pour les frises de la page (petit côté de 72 px). Refaites
/// seulement si le rush ou le format changent (frames/.cle). Renvoie leur description pour la page.
pub fn timeline_strip(source: &Path, out_dir: &Path, width: i64, height: i64, ffmpeg: &Path) -> Value {
    let folder = out_dir.join("frames");
    let (w, h) = if width >= height {
        (round_int(72.0 * width as f64 / height as f64), 72)
    } else {
        (72, round_int(72.0 * height as f64 / width as f64))
    };
    let size = fs::metadata(source).map(|m| m.len()).unwrap_or(0);
    let mt = util::mtime_ns(source).unwrap_or(0);
    let key = format!("{size}|{mt}|{w}x{h}|{STRIP_STEP}|{STRIP_TILE}");
    let key_path = folder.join(".cle");
    let jpgs = |f: &Path| util::list_dir(f).into_iter().filter(|n| n.ends_with(".jpg")).collect::<Vec<_>>();
    let mut sheets = jpgs(&folder);
    if !(!sheets.is_empty() && fs::read_to_string(&key_path).map(|k| k == key).unwrap_or(false)) {
        let part = out_dir.join("frames.part");
        util::remove_all(&part);
        util::mkdirs(&part);
        let fps = util::fmt_g(1.0 / STRIP_STEP, 6);
        let code = procs::call(
            ffmpeg,
            [
                "-v".into(),
                "error".into(),
                "-y".into(),
                "-i".into(),
                source.to_string_lossy().into_owned(),
                "-an".into(),
                "-vf".into(),
                format!("fps={fps},scale={w}:{h},setsar=1,tile={STRIP_TILE}x{STRIP_TILE}"),
                "-q:v".into(),
                "6".into(),
                part.join("s%03d.jpg").to_string_lossy().into_owned(),
            ],
        );
        if code != 0 {
            panic!("ffmpeg (images de la frise) : code {code}");
        }
        let _ = fs::write(part.join(".cle"), &key);
        util::remove_all(&folder);
        let _ = fs::rename(&part, &folder);
        sheets = jpgs(&folder);
    }
    json!({"step": STRIP_STEP, "tile": STRIP_TILE, "w": w, "h": h,
           "sheets": sheets.iter().map(|n| format!("frames/{n}")).collect::<Vec<_>>()})
}

/// Forme d'onde du rush (work/rush_16k.wav) : WAVE_RATE crêtes par seconde, de 0 à 100.
pub fn timeline_wave(work_dir: &Path) -> Option<Vec<i64>> {
    let path = work_dir.join("rush_16k.wav");
    let mut r = hound::WavReader::open(&path).ok()?;
    let rate = r.spec().sample_rate;
    let step = (rate / WAVE_RATE).max(1) as usize;
    let samples: Vec<i32> = r.samples::<i16>().filter_map(Result::ok).map(i32::from).collect();
    // Lu par blocs de step * 2000 images comme la version Python : mêmes fenêtres.
    let mut peaks = vec![];
    for chunk in samples.chunks(step * 2000) {
        for part in chunk.chunks(step) {
            let mx = part.iter().copied().max().unwrap_or(0);
            let mn = part.iter().copied().min().unwrap_or(0);
            peaks.push(mx.max(-mn));
        }
    }
    let top = peaks.iter().copied().max().unwrap_or(1);
    let top = if top == 0 { 1 } else { top } as f64;
    Some(peaks.iter().map(|&p| round_int(100.0 * (p as f64 / top).powf(0.6))).collect())
}

// Formats courants : des proportions à moins de 1 % de l'un d'eux (9:16 recadré puis agrandi…)
// prennent exactement ce format.
pub const COMMON_RATIOS: [(i64, i64); 9] = [(9, 16), (16, 9), (4, 3), (3, 4), (1, 1), (4, 5), (5, 4), (2, 3), (3, 2)];

/// Taille de la version de travail pour une image de w x h affichée : mêmes proportions, petit
/// côté à `short`, dimensions paires (1080x1920 en 9:16, 1920x1080 en 16:9, 1440x1080 en 4:3…).
pub fn work_size(w: i64, h: i64, short: i64) -> (i64, i64) {
    let (mut w, mut h) = (w as f64, h as f64);
    if let Some((a, b)) = COMMON_RATIOS.iter().find(|(a, b)| ((w / h) / (*a as f64 / *b as f64) - 1.0).abs() < 0.01) {
        w = *a as f64;
        h = *b as f64;
    }
    let even = |x: f64| -> i64 { (round_int(x / 2.0) * 2).max(2) };
    let s = short as f64;
    if w <= h {
        (short, even(h * s / w))
    } else {
        (even(w * s / h), short)
    }
}

/// Dimensions d'une version du montage dont le petit côté vaut `short` (« 720x1280 », « 2880x2160 »…).
pub fn res_label(width: i64, height: i64, short: i64) -> String {
    let (w, h) = work_size(width, height, short);
    format!("{w}x{h}")
}

static STREAM_VIDEO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Stream #\d+:\d+.*?: Video: (.*)").unwrap());
static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ ,](\d{2,5})x(\d{2,5})[ ,\[]").unwrap());

/// Taille codée de l'image (sans rotation) lue dans la sortie de « ffmpeg -i ».
fn coded_size(info: &str) -> Option<(i64, i64)> {
    let video = STREAM_VIDEO.captures(info)?;
    let line = format!("{} ", &video[1]);
    let size = SIZE.captures(&line)?;
    Some((size[1].parse().ok()?, size[2].parse().ok()?))
}

/// Format de la composition, celui du rush : (largeur, hauteur) de sa version de travail
/// (1080x1920 en 9:16, 1920x1080 en 16:9, 1440x1080 en 4:3…). build_edit l'écrit dans edit.json
/// (« width », « height »), que lisent le rendu, la page des moments et les téléchargements.
pub fn frame_size(root: &Path) -> (i64, i64) {
    let p = root.join("public").join("rushes").join("rush_1080.mp4");
    if !p.is_file() {
        return (1080, 1920);
    }
    coded_size(&procs::ffmpeg_info(&p)).unwrap_or((1080, 1920))
}

/// Positions de l'habillage en px de la composition (tailles en px, petit côté de 1080, les mêmes
/// dans tous les formats). Image haute (9:16) : sous-titres à 1080 px du haut, texte tapé centré
/// sous 520 px ; image basse (16:9, 4:3) : sous-titres à 310 px du bas, texte tapé sous 120 px.
pub fn frame_layout(width: i64, height: i64) -> Value {
    let _ = width;
    json!({"capTop": 1080.min(height - 310), "typedTop": 120.max(height - 1400)})
}

static SAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"SAR (\d+):(\d+)").unwrap());
static ROT1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"rotation of (-?[\d.]+) degrees").unwrap());
static ROT2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"rotate\s*:\s*(-?\d+)").unwrap());

/// Taille de l'image d'une vidéo telle qu'elle s'affiche (rotation du téléphone et pixels non
/// carrés compris), lue dans la sortie de « ffmpeg -i ».
pub fn display_size(path: &Path) -> Option<(i64, i64)> {
    let out = procs::ffmpeg_info(path);
    display_size_of(&out)
}

pub fn display_size_of(out: &str) -> Option<(i64, i64)> {
    let video = STREAM_VIDEO.captures(out)?;
    let line = video[1].to_string();
    let (mut w, mut h) = coded_size(out)?;
    if let Some(sar) = SAR.captures(&line) {
        let (a, b): (i64, i64) = (sar[1].parse().unwrap_or(0), sar[2].parse().unwrap_or(0));
        if a != 0 && b != 0 && sar[1] != sar[2] {
            w = round_int(w as f64 * a as f64 / b as f64);
        }
    }
    let rot = ROT1.captures(out).or_else(|| ROT2.captures(out));
    if let Some(r) = rot {
        let deg = round_int(r[1].parse::<f64>().unwrap_or(0.0));
        if deg.rem_euclid(180) == 90 {
            std::mem::swap(&mut w, &mut h);
        }
    }
    Some((w, h))
}

// Langue parlée dans le rush (transcription Whisper) : français sauf choix contraire sur la page
// « Rushes et montages » ou « prepare --langue en ». Fixée plutôt que détectée : la détection
// ralentirait chaque morceau transcrit. work/langue.txt : choix pour la prochaine préparation
// (commun à tous les rushes) ; work/langue_rush.txt : langue du rush en cours (rangée avec lui).
pub const LANGUAGES: [(&str, &str); 2] = [("fr", "Français"), ("en", "Anglais")];

pub fn language_label(code: &str) -> Option<&'static str> {
    LANGUAGES.iter().find(|(c, _)| *c == code).map(|(_, l)| *l)
}

pub fn language(work_dir: &Path) -> String {
    language_from(work_dir, "langue.txt")
}

pub fn language_from(work_dir: &Path, name: &str) -> String {
    match fs::read_to_string(work_dir.join(name)) {
        Ok(t) if language_label(t.trim()).is_some() => t.trim().to_string(),
        _ => "fr".into(),
    }
}

// Mode de l'interface locale (bandeau du haut de chaque page) : « montage » (Auto-montage, tout)
// ou « derush » (Dérushage : découper le rush en moments, sans effets, sous-titres ni Claude).
// work/mode.txt, commun à tous les rushes ; build_edit en tire un montage sans habillage.
pub const MODES: [(&str, &str); 2] = [("montage", "Auto-montage"), ("derush", "Dérushage")];

pub fn app_mode(work_dir: &Path) -> String {
    match fs::read_to_string(work_dir.join("mode.txt")) {
        Ok(t) if MODES.iter().any(|(c, _)| *c == t.trim()) => t.trim().to_string(),
        _ => "montage".into(),
    }
}

/// Sous-titres de la version de travail en dérushage (work/derush.json, oui par défaut).
pub fn derush_subtitles(work_dir: &Path) -> bool {
    match read_json(work_dir.join("derush.json")) {
        Some(Value::Object(m)) => m.get("subtitles").map(|v| util::truthy(Some(v))).unwrap_or(true),
        _ => true,
    }
}

static STREAM_AUDIO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Stream #\d+:\d+.*?: Audio: (.*)").unwrap());

/// Pistes son lisibles d'une vidéo : (indice parmi les pistes son, description), lues dans la
/// sortie de « ffmpeg -i ». Une piste que ffmpeg ne sait pas décoder (son spatial « apac » des
/// iPhone récents : « Audio: none ») est écartée.
pub fn audio_streams(path: &Path) -> Vec<(usize, String)> {
    let out = procs::ffmpeg_info(path);
    STREAM_AUDIO
        .captures_iter(&out)
        .enumerate()
        .map(|(i, c)| (i, c[1].to_string()))
        .filter(|(_, d)| !d.starts_with("none"))
        .collect()
}

/// Arguments ffmpeg (après « -i <path> », entrée 0) qui donnent le son du rush : la piste son
/// seule, ou toutes ses pistes mélangées (un micro par personne enregistré sur des pistes
/// séparées : enregistreur, micros sans fil, caméra à deux entrées). Sortie : [<label>].
/// None si la vidéo n'a pas de son.
pub fn audio_map(path: &Path, label: &str) -> Option<Vec<String>> {
    audio_map_of(&audio_streams(path), label)
}

pub fn audio_map_of(streams: &[(usize, String)], label: &str) -> Option<Vec<String>> {
    if streams.is_empty() {
        return None;
    }
    if streams.len() == 1 {
        return Some(vec![
            "-filter_complex".into(),
            format!("[0:a:{}]anull[{label}]", streams[0].0),
            "-map".into(),
            format!("[{label}]"),
        ]);
    }
    // Pistes additionnées (chacune garde son niveau), limiteur contre la saturation.
    let inputs: String = streams.iter().map(|(i, _)| format!("[0:a:{i}]")).collect();
    Some(vec![
        "-filter_complex".into(),
        format!(
            "{inputs}amix=inputs={}:duration=longest:normalize=0,alimiter=limit=0.97:level=0[{label}]",
            streams.len()
        ),
        "-map".into(),
        format!("[{label}]"),
    ])
}

/// (largeur, hauteur) de la composition d'après edit.json (portrait sans indication).
pub fn edit_size(edit: Option<&Value>) -> (i64, i64) {
    let get = |k: &str| -> Option<i64> {
        let v = edit?.get(k)?;
        if !util::truthy(Some(v)) {
            return None;
        }
        v.as_i64().or_else(|| v.as_f64().map(|x| x as i64)).or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    };
    (get("width").unwrap_or(1080), get("height").unwrap_or(1920))
}

fn fnum(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

fn inum(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64))).unwrap_or(0)
}

pub fn gaps(segments: &[Value], total: f64) -> Vec<Value> {
    let mut points = vec![0.0];
    for s in segments {
        points.push(fnum(s, "start"));
        points.push(fnum(s, "end"));
    }
    points.push(total);
    let mut out = vec![];
    let mut i = 0;
    while i + 1 < points.len() {
        let (a, b) = (points[i], points[i + 1]);
        if b - a >= MIN_GAP {
            out.push(json!({"id": GAP_BASE + out.len() as i64, "start": round_to(a, 3), "end": round_to(b, 3)}));
        }
        i += 2;
    }
    out
}

pub fn is_gap(moment_id: i64) -> bool {
    (GAP_BASE..SPLIT_BASE).contains(&moment_id)
}

// --- Moments scindés ou fusionnés (page des moments) ---------------------------------
// work/segments.json (la transcription) ne change jamais. work/decoupage.json note :
//   {"splits": {"<segment>": [indices des mots où commence une nouvelle partie]},
//    "merges": [[id, id, …], …]}   (moments consécutifs fusionnés, ids effectifs)
// Une partie scindée a l'id part_id(segment, mot) ; un moment fusionné garde l'id du premier.
// Les ids ne dépendent que de ces réglages : annuler une scission ou une fusion rend les
// anciens ids, et les réglages de la page qui vont avec.
pub const SPLIT_BASE: i64 = 1_000_000;

pub fn part_id(seg: i64, word: i64) -> i64 {
    SPLIT_BASE + seg * 1000 + word
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decoupage {
    /// segment (texte de son id) -> rangs des mots où commence une nouvelle partie (triés, uniques)
    pub splits: BTreeMap<String, Vec<i64>>,
    pub merges: Vec<Vec<i64>>,
}

fn as_int(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

pub fn load_decoupage(work_dir: &Path) -> Decoupage {
    let Some(Value::Object(d)) = read_json(work_dir.join("decoupage.json")) else {
        return Decoupage::default();
    };
    let mut out = Decoupage::default();
    // L'ordre des clés du fichier est gardé (comme un dict Python) : BTreeMap le trie, ce qui ne
    // change que l'ordre d'écriture, pas les moments.
    if let Some(Value::Object(sp)) = d.get("splits") {
        for (k, v) in sp {
            let set: BTreeSet<i64> = v.as_array().map(|a| a.iter().filter_map(as_int).collect()).unwrap_or_default();
            out.splits.insert(k.clone(), set.into_iter().collect());
        }
    }
    if let Some(Value::Array(ms)) = d.get("merges") {
        for g in ms {
            if let Some(a) = g.as_array() {
                if a.len() > 1 {
                    out.merges.push(a.iter().filter_map(as_int).collect());
                }
            }
        }
    }
    out
}

pub fn decoupage_json(deco: &Decoupage) -> Value {
    let mut splits = Map::new();
    for (k, v) in &deco.splits {
        if !v.is_empty() {
            splits.insert(k.clone(), json!(v));
        }
    }
    let merges: Vec<&Vec<i64>> = deco.merges.iter().filter(|g| g.len() > 1).collect();
    json!({"splits": splits, "merges": merges})
}

pub fn save_decoupage(work_dir: &Path, deco: &Decoupage) {
    let _ = util::write_json(work_dir.join("decoupage.json"), &decoupage_json(deco), Some(1));
}

/// Moments de parole après scissions et fusions, dans l'ordre du rush. Chaque mot garde son
/// origine (« o » : segment, « oi » : rang du mot) ; « origins » : segments d'origine.
pub fn effective_segments(segments: &[Value], deco: &Decoupage) -> Vec<Value> {
    let mut parts: Vec<Option<Value>> = vec![];
    for s in segments {
        let sid = inum(s, "id");
        let words: Vec<Value> = util::arr(s, "words")
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let mut w = w.as_object().cloned().unwrap_or_default();
                w.insert("o".into(), json!(sid));
                w.insert("oi".into(), json!(i));
                Value::Object(w)
            })
            .collect();
        let n = words.len() as i64;
        let ks: Vec<i64> = deco.splits.get(&sid.to_string()).map(|v| v.iter().copied().filter(|&k| 0 < k && k < n).collect()).unwrap_or_default();
        let mut bounds = vec![0];
        bounds.extend(ks);
        bounds.push(n);
        for j in 0..bounds.len() - 1 {
            let ws: Vec<Value> = words[bounds[j] as usize..bounds[j + 1] as usize].to_vec();
            let (first, last) = (j == 0, j == bounds.len() - 2);
            let mut p = Map::new();
            p.insert("id".into(), json!(if first { sid } else { part_id(sid, bounds[j]) }));
            p.insert("start".into(), if first { s["start"].clone() } else { ws[0]["a"].clone() });
            p.insert("end".into(), if last { s["end"].clone() } else { ws[ws.len() - 1]["b"].clone() });
            p.insert(
                "text".into(),
                if first && last {
                    s["text"].clone()
                } else {
                    json!(ws.iter().map(|w| w["w"].as_str().unwrap_or("")).collect::<Vec<_>>().join(" "))
                },
            );
            p.insert("words".into(), Value::Array(ws));
            p.insert("origins".into(), json!([sid]));
            if !(first && last) {
                p.insert("splitFrom".into(), json!(sid));
            }
            parts.push(Some(Value::Object(p)));
        }
    }
    // Tri stable par début (sorted de Python).
    parts.sort_by(|a, b| {
        let fa = fnum(a.as_ref().unwrap(), "start");
        let fb = fnum(b.as_ref().unwrap(), "start");
        fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
    });
    let pos: HashMap<i64, usize> = parts.iter().enumerate().map(|(i, p)| (inum(p.as_ref().unwrap(), "id"), i)).collect();
    for group in &deco.merges {
        let mut idx: Vec<usize> = group.iter().filter_map(|g| pos.get(g).copied()).collect();
        idx.sort();
        if idx.len() < 2 || idx.iter().enumerate().any(|(k, &i)| i != idx[0] + k) {
            continue; // moments disparus ou plus consécutifs : fusion ignorée
        }
        if idx.iter().any(|&i| parts[i].is_none()) {
            continue;
        }
        let members: Vec<Value> = idx.iter().map(|&i| parts[i].clone().unwrap()).collect();
        let merged = json!({
            "id": members[0]["id"].clone(),
            "start": members[0]["start"].clone(),
            "end": members[members.len() - 1]["end"].clone(),
            "text": members.iter().map(|m| m["text"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join(" "),
            "words": members.iter().flat_map(|m| util::arr(m, "words").to_vec()).collect::<Vec<_>>(),
            "origins": members.iter().flat_map(|m| util::arr(m, "origins").to_vec()).collect::<Vec<_>>(),
            "merged": members.iter().map(|m| m["id"].clone()).collect::<Vec<_>>(),
        });
        for &i in &idx {
            parts[i] = None;
        }
        parts[idx[0]] = Some(merged);
    }
    parts.into_iter().flatten().collect()
}

/// Plans sans parole : ceux de la transcription d'origine (ids stables), sauf ceux qu'une
/// fusion a mis à l'intérieur d'un moment.
pub fn effective_gaps(segments: &[Value], effective: &[Value], total: f64) -> Vec<Value> {
    gaps(segments, total)
        .into_iter()
        .filter(|g| !effective.iter().any(|e| fnum(e, "start") <= fnum(g, "start") && fnum(g, "end") <= fnum(e, "end")))
        .collect()
}

/// (segment d'origine, rang du mot) -> (id effectif, rang dans ce moment).
pub fn word_map(effective: &[Value]) -> HashMap<(i64, i64), (i64, i64)> {
    let mut m = HashMap::new();
    for e in effective {
        for (i, w) in util::arr(e, "words").iter().enumerate() {
            m.insert((inum(w, "o"), inum(w, "oi")), (inum(e, "id"), i as i64));
        }
    }
    m
}

/// (segments d'origine, moments effectifs, découpage).
pub fn load_segments(work_dir: &Path) -> Result<(Vec<Value>, Vec<Value>, Decoupage), String> {
    let raw = match read_json(work_dir.join("segments.json")) {
        Some(Value::Array(a)) => a,
        _ => return Err(format!("{} illisible", work_dir.join("segments.json").display())),
    };
    let deco = load_decoupage(work_dir);
    let eff = effective_segments(&raw, &deco);
    Ok((raw, eff, deco))
}

static WORDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w'’-]+").unwrap());

fn words_of(text: &str) -> Vec<String> {
    WORDS.find_iter(&text.to_lowercase()).map(|m| m.as_str().to_string()).collect()
}

/// Vrai si la phrase du segment en position i est reprise dans l'un des 6 suivants.
pub fn is_retake(segments: &[Value], i: usize) -> bool {
    let wi = words_of(segments[i]["text"].as_str().unwrap_or(""));
    if wi.len() < 2 {
        return false;
    }
    for j in i + 1..(i + 7).min(segments.len()) {
        let wj = words_of(segments[j]["text"].as_str().unwrap_or(""));
        let head_i: Vec<&String> = wi.iter().take(3).collect();
        let head_j: Vec<&String> = wj.iter().take(3).collect();
        let cut = &wj[..wj.len().min(wi.len() + 3)];
        if head_i == head_j || SequenceMatcher::new(&wi, cut).ratio() > 0.6 {
            return true;
        }
    }
    false
}

/// Moments suggérés : work/suggestions.json (écrit par Claude) s'il existe, sinon tout sauf les
/// 1res prises (phrase reprise juste après). Renvoie (ids, raisons).
pub fn suggested_keep(segments: &[Value], work_dir: &Path) -> (HashSet<i64>, BTreeMap<i64, Value>) {
    let path = work_dir.join("suggestions.json");
    if path.exists() {
        let suggest = read_json(&path).unwrap_or(Value::Null);
        let keep: HashSet<i64> = util::arr(&suggest, "keep").iter().filter_map(as_int).collect();
        let mut reasons: BTreeMap<i64, Value> = BTreeMap::new();
        if let Some(Value::Object(r)) = suggest.get("reasons") {
            for (k, v) in r {
                if let Ok(k) = k.trim().parse::<i64>() {
                    reasons.insert(k, v.clone());
                }
            }
        }
        if segments.iter().any(|s| s.get("origins").is_some()) {
            // moments scindés ou fusionnés : ids d'origine
            let (mut ek, mut er) = (HashSet::new(), BTreeMap::new());
            for s in segments {
                let sid = inum(s, "id");
                let origins: Vec<i64> = match s.get("origins").and_then(Value::as_array) {
                    Some(a) => a.iter().filter_map(as_int).collect(),
                    None => vec![sid],
                };
                if origins.iter().any(|o| keep.contains(o)) {
                    ek.insert(sid);
                } else if let Some(r) = origins.first().and_then(|o| reasons.get(o)) {
                    er.insert(sid, r.clone());
                }
            }
            return (ek, er);
        }
        return (keep, reasons);
    }
    let (mut keep, mut reasons) = (HashSet::new(), BTreeMap::new());
    for (pos, s) in segments.iter().enumerate() {
        if is_retake(segments, pos) {
            reasons.insert(inum(s, "id"), json!("1re prise, reprise juste après"));
        } else {
            keep.insert(inum(s, "id"));
        }
    }
    (keep, reasons)
}

/// Effets sonores fournis par défaut (public/sfx/<fichier>, synthétisés par make_sfx) :
/// (identifiant, libellé, fichier, volume de base). « clavier » enchaîne key-1..4.wav.
pub const SOUNDS: [(&str, &str, Option<&str>, f64); 12] = [
    ("cash", "Caisse", Some("cash.wav"), 0.55),
    ("bop", "Bop", Some("bop.wav"), 0.7),
    ("clavier", "Clavier", None, 0.45),
    ("whoosh", "Whoosh", Some("whoosh.wav"), 0.6),
    ("pop", "Pop", Some("pop.wav"), 0.6),
    ("ding", "Ding", Some("ding.wav"), 0.5),
    ("boum", "Boum", Some("boum.wav"), 0.75),
    ("montee", "Montée", Some("montee.wav"), 0.5),
    ("glitch", "Glitch", Some("glitch.wav"), 0.45),
    ("photo", "Déclencheur photo", Some("photo.wav"), 0.6),
    ("faux", "Faux (buzzer)", Some("faux.wav"), 0.45),
    ("juste", "Juste (carillon)", Some("juste.wav"), 0.5),
];

/// Effets visuels proposés sur chaque moment (page des moments, build_edit, rendu).
pub const VISUALS: [(&str, &str); 9] = [
    ("typed", "Texte tapé"),
    ("chip", "Pastille"),
    ("zoomin", "Zoom avant"),
    ("zoomsec", "Zoom sec"),
    ("dezoom", "Dézoom"),
    ("shake", "Secousse"),
    ("flash", "Flash blanc"),
    ("highlight", "Mot mis en valeur"),
    ("zoomlibre", "Zoom libre"),
];

/// Voix modifiées (effet « voix » d'un moment, champ « voice ») : identifiant, libellé, description.
/// Filtres ffmpeg de chaque voix : voice_filter() (build_edit et tests/make_voice_demo.py) ;
/// l'aperçu de la page des moments les imite avec Web Audio (selection/page.html, voiceChain).
pub const VOICES: [(&str, &str, &str); 11] = [
    ("tremblement", "Tremblement", "la voix tremble, avec un peu de robot (moment gêné ou drôle)"),
    ("robot", "Robot", "bourdonnement électronique et résonance métallique"),
    ("ecureuil", "Écureuil", "voix très aiguë, façon dessin animé"),
    ("lutin", "Lutin", "voix plus aiguë, légèrement doublée"),
    ("geant", "Géant", "voix grave, avec un peu d'écho"),
    ("megaphone", "Mégaphone", "voix serrée et saturée, comme dans un porte-voix"),
    ("telephone", "Téléphone", "voix étroite, légèrement numérique"),
    ("salle", "Grande salle", "écho de pièce vide"),
    ("batterie", "Batterie faible", "la voix ralentit et descend, comme une cassette qui s'épuise"),
    ("radio", "Vieille radio", "voix étroite, avec du souffle"),
    ("choeur", "Chœur", "la voix doublée, comme plusieurs personnes"),
];

pub fn voice_ids() -> Vec<&'static str> {
    VOICES.iter().map(|v| v.0).collect()
}

/// Voix faites par HyperFrames au rendu et dans l'aperçu de la page (selection/hf_audio.js,
/// même liste) ; les autres sont faites par ffmpeg dans build_edit (voice_filter).
pub const HF_VOICES: [&str; 7] = ["megaphone", "telephone", "salle", "ecureuil", "lutin", "geant", "choeur"];

pub fn is_hf_voice(v: &str) -> bool {
    HF_VOICES.contains(&v)
}

/// Copie `src` vers `dst` en gardant sa date (`shutil.copy2`).
pub fn copy2(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::copy(src, dst)?;
    if let Ok(m) = fs::metadata(src).and_then(|m| m.modified()) {
        if let Ok(f) = fs::OpenOptions::new().write(true).open(dst) {
            let _ = f.set_modified(m);
        }
    }
    Ok(())
}

/// Fichiers de l'aperçu des voix HyperFrames de la page des moments, copiés dans
/// <out_dir>/hf/ (s'ils manquent ou ont changé) : selection/hf_audio.js, le lecteur et le
/// moteur de HyperFrames, GSAP. Sans node_modules, la page garde l'imitation Web Audio.
pub fn hf_preview_files(out_dir: &Path) {
    let root = paths::root();
    let nm = root.join("node_modules");
    let files: [(&str, PathBuf); 4] = [
        ("hf_audio.js", root.join("selection").join("hf_audio.js")),
        ("hyperframes-player.global.js", nm.join("hyperframes").join("dist").join("hyperframes-player.global.js")),
        ("hyperframe.runtime.iife.js", nm.join("hyperframes").join("dist").join("hyperframe.runtime.iife.js")),
        ("gsap.min.js", nm.join("gsap").join("dist").join("gsap.min.js")),
    ];
    util::mkdirs(out_dir.join("hf"));
    for (name, src) in files {
        let dst = out_dir.join("hf").join(name);
        if !src.exists() {
            continue;
        }
        let changed = !dst.exists()
            || util::mtime(&dst).unwrap_or(0.0) < util::mtime(&src).unwrap_or(0.0)
            || fs::metadata(&dst).map(|m| m.len()).ok() != fs::metadata(&src).map(|m| m.len()).ok();
        if changed {
            let _ = copy2(&src, &dst);
        }
    }
}

/// Graphe ffmpeg (-filter_complex) d'une voix modifiée : entrée [0:a], sortie [r], même durée.
/// Les voix de HF_VOICES ne servent plus qu'aux exemples de /rush/ (tests/make_voice_demo.py) :
/// au rendu, ce sont celles de selection/hf_audio.js.
/// Force 1 = réglage d'origine, jusqu'à 2. « batterie » : le ralentissement se fait avant
/// (batterie_warp), ce filtre n'ajoute que le son fatigué.
pub fn voice_filter(voice: &str, force: f64) -> String {
    let f = force.clamp(0.0, 2.0);
    let ring = |w: f64| -> String {
        format!(
            "asplit[d][m];sine=f=60:r=48000[s];[m][s]amultiply,volume=2.2[w];[d][w]amix=inputs=2:weights=1 {w:.3}:normalize=0:duration=first"
        )
    };
    match voice {
        "robot" => format!("[0:a]{},aecho=0.8:0.8:7:{:.3}[r]", ring((0.55 * f).min(0.9)), (0.35 * f).min(0.6)),
        "ecureuil" => format!("[0:a]rubberband=pitch={:.3}:formant=shifted[r]", 1.0 + 0.6 * f),
        "lutin" => format!(
            "[0:a]rubberband=pitch={:.3}:formant=preserved,chorus=0.6:0.8:30|45:0.3|0.25:0.3|0.4:1.5|2[r]",
            1.0 + 0.3 * f
        ),
        "geant" => format!("[0:a]rubberband=pitch={:.3}:formant=shifted,aecho=0.8:0.6:60:0.2[r]", 1.0 / (1.0 + 0.47 * f)),
        "megaphone" => format!(
            "[0:a]highpass=f=600,lowpass=f=3200,volume={:.2},asoftclip=type=atan,volume=0.5,aecho=0.8:0.4:12:0.25[r]",
            2.0 + 4.0 * f
        ),
        "telephone" => format!(
            "[0:a]highpass=f=400,lowpass=f=3000,acrusher=bits=9:mode=log:mix={:.3},volume=1.6[r]",
            (0.35 * f).min(1.0)
        ),
        "salle" => {
            let d: Vec<f64> = [0.45, 0.3, 0.2].iter().map(|x| (x * f).min(0.9)).collect();
            format!("[0:a]aecho=0.8:0.7:90|170|260:{:.3}|{:.3}|{:.3},lowpass=f=7000[r]", d[0], d[1], d[2])
        }
        "batterie" => "[0:a]acrusher=bits=11:mode=log:mix=0.3,lowpass=f=5000[r]".into(),
        "radio" => format!(
            "[0:a]highpass=f=500,lowpass=f=2800,volume=2,asoftclip[v];anoisesrc=c=brown:a={:.4}:r=48000,highpass=f=1000[nz];[v][nz]amix=inputs=2:duration=first:normalize=0[r]",
            0.02 * f
        ),
        "choeur" => {
            let dt = 0.015 * f;
            format!(
                "[0:a]asplit=3[a][b][c];[b]rubberband=pitch={:.4},adelay=18[b2];[c]rubberband=pitch={:.4},adelay=31[c2];[a][b2][c2]amix=inputs=3:normalize=0:duration=first,volume=0.6[r]",
                1.0 - dt,
                1.0 + dt
            )
        }
        // Tremblement (par défaut) : vibrato et trémolo plus marqués qu'avant, avec un peu de robot.
        _ => format!(
            "[0:a]vibrato=f=8:d={:.3},tremolo=f=10:d={:.3},{},aecho=0.8:0.8:7:{:.3}[r]",
            (0.8 * f).min(1.0),
            (0.6 * f).min(1.0),
            ring((0.3 * f).min(0.6)),
            (0.25 * f).min(0.5)
        ),
    }
}

/// Batterie faible : lecture qui ralentit (hauteur et vitesse baissent ensemble, comme une
/// cassette) ; renvoie le signal ralenti (plus long), à remettre à la durée d'origine avec
/// rubberband=tempo (hauteur gardée).
pub fn batterie_warp(x: &[f64], force: f64) -> Vec<f64> {
    let n = x.len();
    if n < 2 {
        return x.to_vec();
    }
    let k = (0.45 * force.max(0.0)).min(0.8);
    let mut pos = Vec::with_capacity(n);
    let mut acc = 0.0;
    for i in 0..n {
        let t = i as f64 / n as f64;
        acc += 1.0 - k * t.powf(1.6);
        if acc < (n - 1) as f64 {
            pos.push(acc);
        }
    }
    // np.cumsum puis filtre pos < n - 1 : les positions croissent, on garde le début.
    let len = pos.len();
    pos.iter()
        .enumerate()
        .map(|(i, &p)| {
            let i0 = p.floor() as usize;
            let frac = p - i0 as f64;
            let v = if i0 + 1 < n { x[i0] * (1.0 - frac) + x[i0 + 1] * frac } else { x[n - 1] };
            v * (1.0 - 0.25 * i as f64 / len.max(1) as f64)
        })
        .collect()
}

// Effets écartés de la page des moments (work/effets_page.json, « hidden ») : tous les effets
// fournis par défaut le sont tant que l'utilisateur ne les a pas cochés. « defauts » note la
// dernière liste appliquée : un fichier plus ancien reçoit les effets écartés depuis, en plus
// de ses propres choix (2 : sons par défaut ; 3 : voix modifiée, volume de la voix par moment
// et effets visuels).
pub const DEFAULTS_VERSION: i64 = 3;

pub fn default_hidden() -> Vec<(i64, Vec<&'static str>)> {
    let mut v3 = vec!["voix", "gainvoix"];
    v3.extend(VISUALS.iter().map(|v| v.0));
    vec![(2, SOUNDS.iter().map(|s| s.0).collect()), (3, v3)]
}

pub fn hidden_effects(path: &Path) -> Vec<String> {
    let data = match read_json(path) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let mut hidden: BTreeSet<String> = data
        .get("hidden")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|h| h.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let done = match data.get("defauts") {
        Some(Value::Number(n)) if n.is_i64() => n.as_i64().unwrap(),
        _ => 0,
    };
    for (version, ids) in default_hidden() {
        if done < version {
            hidden.extend(ids.iter().map(|s| s.to_string()));
        }
    }
    hidden.into_iter().collect()
}

/// Durée d'un fichier WAV en secondes (0 s'il est illisible).
pub fn wav_len(path: &Path) -> f64 {
    match hound::WavReader::open(path) {
        Ok(r) => round_to(r.duration() as f64 / r.spec().sample_rate as f64, 3),
        Err(_) => 0.0,
    }
}

pub fn sfx_dir() -> PathBuf {
    paths::public().join("sfx")
}

/// Catalogue des sons par défaut pour la page des moments (+ « voix », traitée à part), avec
/// la durée de chaque son (« len », durée d'une répétition ; clavier : une frappe).
pub fn sound_catalog() -> Vec<Value> {
    let mut out: Vec<Value> = SOUNDS
        .iter()
        .map(|(i, label, f, vol)| {
            let len = match f {
                Some(f) => wav_len(&sfx_dir().join(f)),
                None => 0.08,
            };
            json!({"id": i, "label": label, "vol": vol, "len": len})
        })
        .collect();
    out.insert(3, json!({"id": "voix", "label": "Voix modifiée"}));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(work_size(1080, 1920, 1080), (1080, 1920));
        assert_eq!(work_size(3840, 2160, 1080), (1920, 1080));
        assert_eq!(work_size(1440, 1080, 1080), (1440, 1080));
        assert_eq!(work_size(1000, 1000, 720), (720, 720));
        assert_eq!(res_label(1920, 1080, 2160), "3840x2160");
    }

    #[test]
    fn display_size_rotation() {
        let info = "  Stream #0:0[0x1](und): Video: h264 (High), yuv420p(tv, bt709), 1920x1080 [SAR 1:1 DAR 16:9], 30 fps\n    Side data:\n      displaymatrix: rotation of -90.00 degrees\n";
        assert_eq!(display_size_of(info), Some((1080, 1920)));
    }

    #[test]
    fn split_and_merge() {
        let segs: Vec<Value> = serde_json::from_str(r#"[
          {"id": 0, "start": 0.0, "end": 2.0, "text": "a b c", "words": [{"w":"a","a":0.0,"b":0.5},{"w":"b","a":0.6,"b":1.0},{"w":"c","a":1.2,"b":2.0}]},
          {"id": 1, "start": 3.0, "end": 4.0, "text": "d", "words": [{"w":"d","a":3.0,"b":4.0}]}]"#).unwrap();
        let mut deco = Decoupage::default();
        deco.splits.insert("0".into(), vec![2]);
        let eff = effective_segments(&segs, &deco);
        assert_eq!(eff.len(), 3);
        assert_eq!(eff[1]["id"], json!(part_id(0, 2)));
        assert_eq!(eff[1]["text"], json!("c"));
        deco.merges.push(vec![part_id(0, 2), 1]);
        let eff = effective_segments(&segs, &deco);
        assert_eq!(eff.len(), 2);
        assert_eq!(eff[1]["merged"], json!([part_id(0, 2), 1]));
        assert_eq!(eff[1]["end"], json!(4.0));
    }
}
