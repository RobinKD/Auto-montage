//! Prépare la page de sélection des moments (cases à cocher) pour le rush en cours.
//!
//! Entrées : work/segments.json (segments de parole transcrits, par prepare),
//!           work/rush.txt, et work/suggestions.json si Claude en a écrit une
//!           ({"keep": [ids], "reasons": {"id": "raison"}}).
//! Sorties : work/moments/index.html     page à publier (gabarit selection/page.html)
//!           work/moments/clips/*.mp4    un extrait vidéo par moment (360x640, avec le son)
//!           work/moments/thumbs/*.jpg   une vignette par moment
//!           work/moments/frames/*.jpg   images du rush toutes les 0,5 s (planches de 10 x 10), frises
//!           work/moments/montage.mp4    la version de travail actuelle (out/montage_apercu.mp4,
//!                                       compressée pour la page), si elle existe
//! Le bouton « Générer la version de travail » de la page programme dans ~1 min la routine dont
//! l'id est dans work/regen_trigger.txt (connecteur Claude Code Remote, outil update_trigger).
//!
//! Sans work/suggestions.json, la pré-sélection est automatique : un segment dont la phrase
//! est reprise juste après est une 1re prise (retirée, on garde la 2e) ; le reste est gardé.

use crate::fonts_lib;
use crate::moments_lib::{
    edit_size, effective_gaps, frame_layout, hf_preview_files, load_segments, res_label, rush_duration, sound_catalog, suggested_keep,
    timeline_strip, timeline_wave, wav_len, SOUNDS, VOICES,
};
use crate::paths;
use crate::procs;
use crate::progress::{report, report_every};
use crate::pyjson;
use crate::util::{self, num, read_json, round_to, truthy};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// Temps dans le rush tel que Python le garde : flottant, ou entier (`max(0, …)` rend l'entier 0).
#[derive(Clone, Copy)]
struct T {
    x: f64,
    int: bool,
}

impl T {
    fn of(v: &Value) -> T {
        T { x: v.as_f64().unwrap_or(0.0), int: v.is_i64() || v.is_u64() }
    }
}

/// `round(v, n)` de Python sur un nombre JSON : un entier reste un entier.
fn py_round(v: &Value, n: usize) -> Value {
    if v.is_i64() || v.is_u64() {
        return v.clone();
    }
    num(round_to(v.as_f64().unwrap_or(0.0), n))
}

/// `round(v - a, n)` : entier seulement si les deux le sont.
fn sub_round(v: &Value, a: T, n: usize) -> Value {
    if (v.is_i64() || v.is_u64()) && a.int {
        return json!(v.as_i64().unwrap_or(0) - a.x as i64);
    }
    num(round_to(v.as_f64().unwrap_or(0.0) - a.x, n))
}

/// Égalité de deux listes de bornes comme en Python (0 == 0.0).
fn same_sig(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_sig(p, q)),
        (Value::Number(p), Value::Number(q)) => p.as_f64() == q.as_f64(),
        _ => a == b,
    }
}

/// `html.escape(text)` de Python (guillemets compris).
pub(crate) fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

/// `str.capitalize()` : première lettre en majuscule, le reste en minuscules.
fn capitalize(text: &str) -> String {
    let mut c = text.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// `os.path.getmtime` exactement comme Python (secondes + nanosecondes × 1e-9), pour que les dates
/// et les clés écrites soient les mêmes.
pub(crate) fn py_mtime(path: &Path) -> Option<f64> {
    let ns = util::mtime_ns(path)?;
    Some((ns / 1_000_000_000) as f64 + (ns % 1_000_000_000) as f64 * 1e-9)
}

/// `datetime.fromtimestamp(t, timezone.utc)` (arrondi à la microseconde, moitié au pair).
pub(crate) fn utc_of(t: f64) -> chrono::DateTime<chrono::Utc> {
    let secs = t.floor();
    let mut micros = ((t - secs) * 1e6).round_ties_even() as i64;
    let mut secs = secs as i64;
    if micros >= 1_000_000 {
        secs += 1;
        micros -= 1_000_000;
    }
    chrono::DateTime::from_timestamp(secs, (micros * 1000) as u32).unwrap_or_default()
}

/// `.isoformat()` d'une date UTC (microsecondes omises si nulles).
pub(crate) fn iso(d: &chrono::DateTime<chrono::Utc>) -> String {
    if d.timestamp_subsec_micros() == 0 {
        d.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    } else {
        d.format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()
    }
}

/// `datetime.fromtimestamp(t, timezone.utc).isoformat()`
pub(crate) fn iso_utc(t: f64) -> String {
    iso(&utc_of(t))
}

/// `os.devnull`
pub(crate) fn devnull() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

fn ffmpeg_ok(args: &[String], what: &str) -> Result<(), String> {
    let code = procs::call(paths::ffmpeg(), args);
    if code != 0 {
        return Err(format!("ffmpeg ({what}) : code {code}"));
    }
    Ok(())
}

fn s(x: impl Into<String>) -> String {
    x.into()
}

fn p(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct Page {
    out: PathBuf,
    width: i64,
    height: i64,
    small: String,
    source: PathBuf,
    sig_path: PathBuf,
    signatures: Map<String, Value>,
    made: usize, // extraits refaits par cette page
    face: Vec<Value>,
    persons: Vec<(i64, String)>,
}

impl Page {
    /// Extrait vidéo et vignette d'un moment ; gardés s'ils existent déjà pour les mêmes bornes.
    fn extract(&mut self, name: &str, a: T, b: T, crf: i64, abr: &str) -> Result<(), String> {
        let ra = if a.int { json!(a.x as i64) } else { num(round_to(a.x, 3)) };
        let rb = if b.int { json!(b.x as i64) } else { num(round_to(b.x, 3)) };
        let mut sig = vec![ra, rb];
        if (self.width, self.height) != (1080, 1920) {
            sig.push(json!(format!("{}x{}", self.width, self.height)));
        }
        let sig = Value::Array(sig);
        let clip = self.out.join("clips").join(format!("{name}.mp4"));
        let thumb = self.out.join("thumbs").join(format!("{name}.jpg"));
        if self.signatures.get(name).map(|v| same_sig(v, &sig)).unwrap_or(false) && clip.exists() && thumb.exists() {
            return Ok(());
        }
        // Noms provisoires renommés une fois complets ; liste des bornes enregistrée régulièrement :
        // une préparation mise en pause ou coupée ne refait que les extraits manquants.
        let part_clip = self.out.join("clips").join(format!("{name}.part.mp4"));
        let part_thumb = self.out.join("thumbs").join(format!("{name}.part.jpg"));
        let src = p(&self.source);
        ffmpeg_ok(
            &[
                s("-v"), s("error"), s("-y"), s("-ss"), format!("{:.3}", a.x), s("-to"), format!("{:.3}", b.x), s("-i"), src.clone(),
                s("-vf"), self.small.clone(), s("-c:v"), s("libx264"), s("-preset"), s("veryfast"), s("-crf"), crf.to_string(),
                s("-pix_fmt"), s("yuv420p"), s("-c:a"), s("aac"), s("-b:a"), s(abr), s("-ac"), s("1"), s("-movflags"), s("+faststart"),
                p(&part_clip),
            ],
            "extrait",
        )?;
        ffmpeg_ok(
            &[
                s("-v"), s("error"), s("-y"), s("-ss"), format!("{:.3}", (a.x + b.x) / 2.0), s("-i"), src, s("-frames:v"), s("1"),
                s("-vf"), s("scale=120:-2"), s("-q:v"), s("5"), p(&part_thumb),
            ],
            "vignette",
        )?;
        fs::rename(&part_clip, &clip).map_err(|e| e.to_string())?;
        fs::rename(&part_thumb, &thumb).map_err(|e| e.to_string())?;
        self.signatures.insert(name.to_string(), sig);
        self.made += 1;
        if self.made % 10 == 0 {
            let part = self.sig_path.with_file_name(".bornes.json.part");
            let _ = util::write_json(&part, &Value::Object(self.signatures.clone()), None);
            let _ = fs::rename(&part, &self.sig_path);
        }
        Ok(())
    }

    fn face_at(&self, t: f64) -> Value {
        // min() de Python : le premier des plus proches.
        let mut best: Option<&Value> = None;
        let mut bd = f64::INFINITY;
        for f in &self.face {
            let d = (util::fd(f, "t", 0.0) - t).abs();
            if d < bd {
                bd = d;
                best = Some(f);
            }
        }
        match best {
            Some(f) => json!([f.get("x").cloned().unwrap_or(Value::Null), f.get("y").cloned().unwrap_or(Value::Null)]),
            None => json!([0.5, 0.4]),
        }
    }

    /// Cadrage pendant l'extrait [a, b] (detect_face) : [[temps dans l'extrait, x, y, zoom
    /// maximal, visages [[personne, x, y, w, h], …]], …], suivi par l'aperçu de la page comme par le rendu.
    fn face_track(&self, a: T, b: f64) -> Value {
        Value::Array(
            self.face
                .iter()
                .filter(|f| {
                    let t = util::fd(f, "t", 0.0);
                    a.x - 0.5 <= t && t <= b + 0.5
                })
                .map(|f| {
                    json!([
                        sub_round(f.get("t").unwrap_or(&json!(0)), a, 2),
                        f.get("x").cloned().unwrap_or(Value::Null),
                        f.get("y").cloned().unwrap_or(Value::Null),
                        f.get("s").cloned().unwrap_or(json!(3)),
                        f.get("faces").cloned().unwrap_or(json!([])),
                    ])
                })
                .collect(),
        )
    }

    /// Personnes vues pendant [a, b] (au moins 2 instants), de la plus présente à la moins présente :
    /// proposées pour le cadrage du moment (« Cadrage » de la page).
    fn persons_in(&self, a: f64, b: f64) -> Value {
        let mut seen: Vec<(Value, i64)> = vec![];
        for f in &self.face {
            let t = util::fd(f, "t", 0.0);
            if a <= t && t <= b {
                for one in util::arr(f, "faces") {
                    let k = one.get(0).cloned().unwrap_or(Value::Null);
                    match seen.iter_mut().find(|(x, _)| same_sig(x, &k)) {
                        Some(e) => e.1 += 1,
                        None => seen.push((k, 1)),
                    }
                }
            }
        }
        seen.sort_by_key(|(_, n)| -n); // tri stable, comme sorted()
        Value::Array(
            seen.into_iter()
                .filter(|(k, n)| *n >= 2 && k.as_i64().map(|k| self.persons.iter().any(|(p, _)| *p == k)).unwrap_or(false))
                .map(|(k, _)| k)
                .collect(),
        )
    }
}

pub fn main(_args: &[String]) -> i32 {
    match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Erreur : {e}");
            1
        }
    }
}

fn run() -> Result<(), String> {
    let root = paths::root();
    let work = paths::work();
    let out = work.join("moments");

    // Moments de parole : transcription, avec les scissions et fusions faites sur la page
    // (work/decoupage.json, moments_lib::effective_segments).
    let (raw_segments, segments, _deco) = load_segments(&work)?;
    let rush = util::read_text(work.join("rush.txt")).ok_or("work/rush.txt illisible")?;

    let (keep, reasons) = suggested_keep(&segments, &work);

    let edit_path = paths::data().join("edit.json");
    let edit_data = if edit_path.exists() { read_json(&edit_path).ok_or("src/data/edit.json illisible")? } else { json!({}) };
    let overlay = edit_data.get("overlayDefaults").cloned().unwrap_or(json!({}));
    let face = match read_json(paths::data().join("face.json")) {
        Some(Value::Array(a)) => a,
        _ => return Err("src/data/face.json illisible".into()),
    };
    let (width, height) = edit_size(Some(&edit_data)); // format du rush (9:16, 16:9, 4:3…)
    // Extraits (petit côté de 360) et version de travail pour la page (720) : 360x640 et 720x1280 en 9:16.
    let small = format!("scale={}", res_label(width, height, 360).replace('x', ":"));
    let working_scale = format!("scale={}", res_label(width, height, 720).replace('x', ":"));

    // Vignettes des personnes (detect_face : work/visages/p<n>.jpg), copiées pour la page.
    let mut persons: Vec<(i64, String)> = vec![];
    util::mkdirs(out.join("visages"));
    for name in util::list_dir(out.join("visages")) {
        let _ = fs::remove_file(out.join("visages").join(name));
    }
    for name in util::list_dir(work.join("visages")) {
        let digits = name.strip_prefix('p').and_then(|n| n.strip_suffix(".jpg")).unwrap_or("");
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            fs::copy(work.join("visages").join(&name), out.join("visages").join(&name)).map_err(|e| e.to_string())?;
            persons.push((digits.parse().unwrap_or(0), format!("visages/{name}")));
        }
    }

    for sub in ["clips", "thumbs"] {
        util::mkdirs(out.join(sub));
    }
    let source = paths::rushes().join("rush_1080.mp4"); // H.264, lisible partout
    let mut moments: Vec<Value> = vec![];
    // Avancement en secondes de vidéo : durée des extraits faits (moments puis plans sans parole),
    // puis la version de travail pour la page (deux passes, chacune comptée pour la durée du montage).
    let gap_list = effective_gaps(&raw_segments, &segments, rush_duration(&work));
    // Extraits déjà faits pour les mêmes bornes : gardés (une scission ou une fusion ne refait
    // que les moments touchés). clips/.bornes.json : nom -> [début, fin].
    let sig_path = out.join("clips").join(".bornes.json");
    let signatures = match read_json(&sig_path) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let mut pg = Page { out: out.clone(), width, height, small, source: source.clone(), sig_path: sig_path.clone(), signatures, made: 0, face, persons };

    let preview = paths::out().join("montage_apercu.mp4");
    let fps = edit_data.get("fps").cloned().unwrap_or(Value::Null);
    let montage_s = if preview.exists() && truthy(Some(&fps)) {
        util::fd(&edit_data, "durationInFrames", 0.0) / fps.as_f64().unwrap_or(1.0)
    } else {
        0.0
    };
    let fnum = |v: &Value, k: &str| util::fd(v, k, 0.0);
    let steps_total: f64 = segments.iter().map(|s| fnum(s, "end") - fnum(s, "start") + 0.1).sum::<f64>()
        + gap_list.iter().map(|g| fnum(g, "end") - fnum(g, "start")).sum::<f64>()
        + 2.0 * montage_s;
    let mut steps_done = 0.0;
    // Tours de parole (diarize) : cadrage « personne qui parle » de l'aperçu, même règle que build_edit.
    let turns = crate::diarize::turns(&work);
    let turns_json = |a: f64, b: f64| -> Value {
        Value::Array(
            crate::diarize::turns_in(&turns, a, b)
                .into_iter()
                .map(|(x, y, person, spk)| json!([num(round_to(x - a, 2)), num(round_to(y - a, 2)), person, spk]))
                .collect(),
        )
    };
    const LABEL: &str = "Découpage des moments";
    report(LABEL, 0.0, if steps_total != 0.0 { steps_total } else { 1.0 });
    let empty = json!({});
    for seg in &segments {
        let sid = seg["id"].as_i64().unwrap_or(0);
        let name = format!("m_{sid:03}");
        let start = fnum(seg, "start");
        let end = fnum(seg, "end");
        let a = if start - 0.05 > 0.0 { T { x: start - 0.05, int: false } } else { T { x: 0.0, int: true } };
        let b = T { x: end + 0.05, int: false };
        pg.extract(&name, a, b, 28, "64k")?;
        // Mots avec leurs temps dans l'extrait (qui commence à « a »), corrections du montage appliquées.
        let fixes = overlay.get("fixes").and_then(|f| f.get(sid.to_string())).unwrap_or(&empty);
        let words: Vec<Value> = util::arr(seg, "words")
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let mut o = json!({
                    "w": fixes.get(i.to_string()).cloned().unwrap_or_else(|| w.get("w").cloned().unwrap_or(Value::Null)),
                    "a": sub_round(w.get("a").unwrap_or(&json!(0)), a, 3),
                    "b": sub_round(w.get("b").unwrap_or(&json!(0)), a, 3),
                });
                if let Some(k) = w.get("spk").filter(|k| k.is_i64()) {
                    o["s"] = k.clone(); // locuteur du mot (diarize)
                }
                o
            })
            .collect();
        let joined: Vec<String> = words
            .iter()
            .filter(|w| truthy(w.get("w")))
            .map(|w| match &w["w"] {
                Value::String(t) => t.clone(),
                other => pyjson::dumps_utf8(other),
            })
            .collect();
        let text = if joined.is_empty() || joined.join(" ").is_empty() { seg.get("text").cloned().unwrap_or(json!("")) } else { json!(joined.join(" ")) };
        let key = sid.to_string();
        let ov = |k: &str| overlay.get(k).and_then(|m| m.get(&key)).cloned();
        let mut m = Map::new();
        m.insert("id".into(), seg["id"].clone());
        m.insert("start".into(), py_round(&seg["start"], 2));
        m.insert("end".into(), py_round(&seg["end"], 2));
        m.insert("text".into(), text);
        m.insert("words".into(), Value::Array(words));
        m.insert("face".into(), pg.face_at((start + end) / 2.0));
        m.insert("faceTrack".into(), pg.face_track(a, b.x));
        m.insert("persons".into(), pg.persons_in(start, end));
        if !turns.is_empty() {
            m.insert("turns".into(), turns_json(a.x, b.x)); // [[début, fin, personne ou -1, locuteur]]
        }
        m.insert("cashWord".into(), ov("cashWord").unwrap_or(Value::Null));
        m.insert("chip".into(), ov("chips").unwrap_or(Value::Null));
        m.insert("vfxExtra".into(), ov("vfx").unwrap_or(json!([]))); // effets visuels placés par Claude
        m.insert("suggested".into(), json!(keep.contains(&sid)));
        m.insert("reason".into(), reasons.get(&sid).cloned().unwrap_or(json!("")));
        m.insert("clip".into(), json!(format!("clips/{name}.mp4")));
        m.insert("thumb".into(), json!(format!("thumbs/{name}.jpg")));
        // Moment coupé (« Couper ») : segment d'origine et rang du mot de coupe.
        if seg.get("splitFrom").map(|v| !v.is_null()).unwrap_or(false) {
            m.insert("splitFrom".into(), seg["splitFrom"].clone());
            m.insert("splitWord".into(), util::arr(seg, "words").first().and_then(|w| w.get("oi")).cloned().unwrap_or(Value::Null));
        }
        // Rang de chaque mot dans son segment d'origine (pour couper ici).
        m.insert(
            "origins".into(),
            Value::Array(
                util::arr(seg, "words")
                    .iter()
                    .map(|w| json!([w.get("o").cloned().unwrap_or(Value::Null), w.get("oi").cloned().unwrap_or(Value::Null)]))
                    .collect(),
            ),
        );
        moments.push(Value::Object(m));
        steps_done += end - start + 0.1;
        report(LABEL, steps_done, steps_total);
    }

    // Plans sans parole (entre les segments) : proposés décochés, à couper à la main sur la page.
    for g in &gap_list {
        let gid = g["id"].as_i64().unwrap_or(0);
        let name = format!("g_{gid:04}");
        let (ga, gb) = (T::of(&g["start"]), T::of(&g["end"]));
        pg.extract(&name, ga, gb, 30, "48k")?;
        moments.push(json!({
            "id": g["id"], "gap": true, "start": g["start"], "end": g["end"], "text": "", "words": [],
            "face": pg.face_at((ga.x + gb.x) / 2.0), "faceTrack": pg.face_track(ga, gb.x),
            "persons": pg.persons_in(ga.x, gb.x), "cashWord": null, "chip": null,
            "suggested": false, "reason": "", "clip": format!("clips/{name}.mp4"), "thumb": format!("thumbs/{name}.jpg"),
        }));
        steps_done += gb.x - ga.x;
        report(LABEL, steps_done, steps_total);
    }
    moments.sort_by(|x, y| fnum(x, "start").partial_cmp(&fnum(y, "start")).unwrap_or(std::cmp::Ordering::Equal));
    util::write_json(&sig_path, &Value::Object(pg.signatures.clone()), None).map_err(|e| e.to_string())?;

    // Version de travail actuelle + correspondance clip du montage -> moment (pour surligner
    // le moment en cours pendant la lecture).
    let mut working = Value::Null;
    if preview.exists() {
        let target = out.join("montage.mp4");
        // Recompressée seulement si la version de travail a changé depuis (scission, fusion…).
        if !target.exists() || py_mtime(&target).unwrap_or(0.0) < py_mtime(&preview).unwrap_or(0.0) {
            let passlog = p(&out.join("pass"));
            let common = vec![
                s("-v"), s("error"), s("-y"), s("-i"), p(&preview), s("-vf"), working_scale.clone(), s("-c:v"), s("libx264"),
                s("-preset"), s("slow"), s("-b:v"), s("2500k"), s("-pix_fmt"), s("yuv420p"),
            ];
            let mut pass1 = common.clone();
            pass1.extend([s("-pass"), s("1"), s("-passlogfile"), passlog.clone(), s("-an"), s("-f"), s("mp4"), s(devnull())]);
            ffmpeg_ok(&pass1, "version de travail, 1re passe")?;
            report_every(LABEL, steps_done + montage_s, steps_total, 0.0);
            let mut pass2 = common;
            pass2.extend([
                s("-pass"), s("2"), s("-passlogfile"), passlog, s("-c:a"), s("aac"), s("-b:a"), s("128k"), s("-movflags"), s("+faststart"),
                p(&target),
            ]);
            ffmpeg_ok(&pass2, "version de travail, 2e passe")?;
            for f in util::list_dir(&out) {
                if f.starts_with("pass") {
                    let _ = fs::remove_file(out.join(f));
                }
            }
        }
        report_every(LABEL, steps_total, steps_total, 0.0);
        let edit = read_json(&edit_path).ok_or("src/data/edit.json illisible")?;
        let fps = util::fd(&edit, "fps", 30.0);
        working = json!({
            "src": "montage.mp4",
            "duration": round_to(util::fd(&edit, "durationInFrames", 0.0) / fps, 2),
            // Heure du rendu (UTC), affichée sur la page pour voir que la version a changé.
            "renderedAt": iso_utc(py_mtime(&preview).unwrap_or(0.0)),
            "clips": util::arr(&edit, "clips").iter().map(|c| json!({
                "seg": c.get("seg").cloned().unwrap_or(Value::Null),
                "start": num(round_to(fnum(c, "from") / fps, 3)),
                "end": num(round_to((fnum(c, "from") + fnum(c, "durationInFrames")) / fps, 3)),
            })).collect::<Vec<_>>(),
            // Sélection et réglages des moments de cette version (build_edit), ou None.
            "page": edit.get("page").cloned().unwrap_or(Value::Null),
        });
    }

    // Effets sonores proposés par moment (les défauts du montage, ex. la caisse sur l'argent).
    let defaults = edit_data.get("sfxDefaults").cloned().unwrap_or(json!({}));
    for m in moments.iter_mut() {
        let key = match &m["id"] {
            Value::Number(n) => n.to_string(),
            other => other.as_str().unwrap_or("").to_string(),
        };
        let sfx = defaults.get(&key).cloned().unwrap_or(json!([]));
        m.as_object_mut().unwrap().insert("sfx".into(), sfx);
    }
    // Sons et police pour l'habillage des moments dans la page.
    util::mkdirs(out.join("sfx"));
    // Sons personnels (public/sfx/perso/<nom>.wav, envoyés depuis la page « Rushes et montages ») :
    // effets « perso-<nom> » en plus de ceux du projet.
    let sfx_dir = paths::public().join("sfx");
    let perso_dir = sfx_dir.join("perso");
    let perso: Vec<String> = util::list_dir(&perso_dir).into_iter().filter_map(|n| n.strip_suffix(".wav").map(String::from)).collect();
    let mut sources: Vec<(String, PathBuf)> = ["key-1", "key-2", "key-3", "key-4"].iter().map(|n| (n.to_string(), sfx_dir.join(format!("{n}.wav")))).collect();
    sources.extend(SOUNDS.iter().filter_map(|(i, _, f, _)| f.map(|f| (i.to_string(), sfx_dir.join(f)))));
    sources.extend(perso.iter().map(|n| (format!("perso-{n}"), perso_dir.join(format!("{n}.wav")))));
    for (name, src) in &sources {
        ffmpeg_ok(
            &[s("-v"), s("error"), s("-y"), s("-i"), p(src), s("-c:a"), s("libmp3lame"), s("-b:a"), s("96k"), p(&out.join("sfx").join(format!("{name}.mp3")))],
            "sons",
        )?;
    }
    // Ordre de os.listdir : celui du dossier (le premier fichier « oliver… » trouvé).
    let fonts_dir = paths::public().join("fonts");
    let oliver: Vec<String> = fs::read_dir(&fonts_dir)
        .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|f| f.to_lowercase().starts_with("oliver")).collect())
        .unwrap_or_default();
    let mut font = Value::Null;
    if let Some(first) = oliver.first() {
        util::mkdirs(out.join("fonts"));
        let ext = Path::new(first).extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default();
        let rel = format!("fonts/oliver{ext}");
        fs::copy(fonts_dir.join(first), out.join("fonts").join(format!("oliver{ext}"))).map_err(|e| e.to_string())?;
        font = json!(rel);
    }

    // Sons par défaut (moments_lib::SOUNDS), voix modifiée (voix qui tremble), puis sons personnels.
    let mut sfx_catalog = sound_catalog();
    for n in &perso {
        sfx_catalog.push(json!({"id": format!("perso-{n}"), "label": capitalize(&n.replace('-', " ")), "vol": 0.6,
                                "len": wav_len(&perso_dir.join(format!("{n}.wav")))}));
    }
    let trigger = util::read_text(work.join("regen_trigger.txt")).map(Value::String).unwrap_or(Value::Null);
    // Page « Téléchargements » (make_downloads), lien affiché sous le lecteur.
    let downloads_page = util::read_text(work.join("downloads_url.txt")).map(Value::String).unwrap_or(Value::Null);

    // Nombre de lignes des sous-titres et taille de chacune (page publiée : sans le style de l'interface locale).
    let caption_lines = fonts_lib::read_style();
    hf_preview_files(&out); // aperçu des voix faites par HyperFrames (hf/)
    // Frises de la page : images du rush toutes les 0,5 s et forme d'onde de la voix.
    let strip = timeline_strip(&source, &out, width, height, paths::ffmpeg());
    let wave_peaks = timeline_wave(&work);
    let page = fs::read_to_string(root.join("selection").join("page.html")).map_err(|e| e.to_string())?;
    let rush_name = Path::new(&rush).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let intro = if truthy(overlay.get("intro")) {
        json!({"text": overlay["intro"], "at": overlay.get("introAt").cloned().unwrap_or(json!(0.2)),
               "type": overlay.get("introType").cloned().unwrap_or(json!(1.6))})
    } else {
        Value::Null
    };
    let mut persons_obj = Map::new();
    for (k, v) in &pg.persons {
        persons_obj.insert(k.to_string(), json!(v));
    }
    let data = json!({
        "rush": rush_name, "width": width, "height": height, "layout": frame_layout(width, height), "moments": moments,
        "working": working, "sfxCatalog": sfx_catalog, "regenTrigger": trigger, "downloadsPage": downloads_page,
        "intro": intro, "captionFont": font,
        "captionLines": {"lines": caption_lines.get("lines").cloned().unwrap_or(Value::Null),
                         "lineSizes": caption_lines.get("lineSizes").cloned().unwrap_or(Value::Null)},
        "persons": persons_obj,
        // Locuteurs (diarize) : [{id, person, seconds}], pastilles de couleur sur la page.
        "speakers": util::arr(&read_json(work.join("locuteurs.json")).unwrap_or(Value::Null), "speakers")
            .iter()
            .map(|sp| json!({"id": sp.get("id").cloned().unwrap_or(Value::Null), "person": sp.get("person").cloned().unwrap_or(Value::Null),
                             "seconds": sp.get("seconds").cloned().unwrap_or(Value::Null)}))
            .collect::<Vec<_>>(),
        "voices": VOICES.iter().map(|(i, label, desc)| json!({"id": i, "label": label, "desc": desc})).collect::<Vec<_>>(),
        "strip": strip, "wave": wave_peaks, "rushEnd": num(round_to(rush_duration(&work), 2)),
    });
    // Nom du rush échappé, données sans « < » (une transcription contenant « </script> » ne
    // referme pas le script de la page).
    let stem = Path::new(&rush_name).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let page = page.replace("__RUSH__", &html_escape(&stem));
    let page = page.replace("/*__DATA__*/null", &pyjson::dumps_utf8(&data).replace('<', "\\u003c"));
    fs::write(out.join("index.html"), page).map_err(|e| e.to_string())?;
    let suggested: Vec<&Value> = moments.iter().filter(|m| m["suggested"] == json!(true)).collect();
    let kept: f64 = suggested.iter().map(|m| fnum(m, "end") - fnum(m, "start")).sum();
    let n_gaps = moments.iter().filter(|m| truthy(m.get("gap"))).count();
    println!(
        "{} moments de parole + {} plans sans parole, {} pré-cochés ({:.1} s) -> work/moments/",
        moments.len() - n_gaps,
        n_gaps,
        suggested.len(),
        kept
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_like_python() {
        assert_eq!(html_escape("a&b<\"'>"), "a&amp;b&lt;&quot;&#x27;&gt;");
        assert_eq!(capitalize("bip ESSAI"), "Bip essai");
        assert_eq!(iso_utc(0.0), "1970-01-01T00:00:00+00:00");
        assert_eq!(iso_utc(1.5), "1970-01-01T00:00:01.500000+00:00");
        assert!(same_sig(&json!([0, 4.09]), &json!([0.0, 4.09])));
        assert_eq!(pyjson::dumps(&sub_round(&json!(1), T { x: 0.0, int: true }, 3)), "1");
        assert_eq!(pyjson::dumps(&sub_round(&json!(1.2345), T { x: 0.5, int: false }, 3)), "0.734");
    }
}
