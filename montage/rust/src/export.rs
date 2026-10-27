//! Préréglages d'export par plateforme : une copie de la vidéo entière rendue pour YouTube,
//! YouTube Shorts, TikTok, Instagram (Reels, fil en 4:5, carré), Facebook, LinkedIn, X et Vimeo.
//! Idée reprise de FilmCraft (crates/export/presets.rs : liste de cibles de diffusion, chacune
//! avec sa taille d'image, son débit et son son) ; réglages choisis ici d'après les
//! recommandations publiées par les plateformes.
//!
//! Chaque préréglage (`PRESETS`) fixe : format et taille d'image, débit vidéo (pour 30 i/s,
//! ×1,5 au-delà) et plafond, débit AAC, images/s au plus, volume cible (LUFS) et crête vraie,
//! durée maximale acceptée. Encodage H.264 High + AAC 48 kHz stéréo, `+faststart` (lecture
//! avant la fin du téléchargement), image clé toutes les 2 s.
//!
//! Source : out/montage_4k.mp4 s'il existe et n'est pas plus ancien que l'aperçu, sinon
//! out/montage_apercu.mp4 (ou `--source`). Format du montage différent de la cible : l'image
//! entière est posée sur un fond flou (la même image agrandie et floutée), rien n'est recadré,
//! donc aucun visage n'est coupé. Durée au-delà du maximum de la plateforme : avertissement, ou
//! vidéo coupée à ce maximum avec `--couper`.
//!
//! Volume : le son de la source est mesuré puis amené à la cible de chaque plateforme
//! (loudness.rs : EBU R128, limiteur à crête vraie), avant l'encodage ; le fichier fini est
//! mesuré de nouveau.
//!
//! Sorties : out/export/<rush>_<préréglage>.mp4 (écrit en *.part.mp4 puis renommé) et le résumé
//! work/exports.json ({"exports": {préréglage: {fichier, taille, durée, LUFS, crête,
//! avertissements…}}}), lu par la page des moments (menu « Télécharger », /api/export) et la
//! page « Téléchargements ».
//!
//! Commande : auto-montage export <préréglage…>|tout [--couper] [--source <fichier>]
//!            auto-montage export --liste [--json]
//! Encodage par la carte graphique si elle marche (gpu.rs), sinon par le processeur.

use crate::gpu::{self, Encoder, Rate};
use crate::loudness;
use crate::paths;
use crate::procs;
use crate::progress::{self, say};
use crate::util;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Une plateforme cible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    /// Usage, en mots d'utilisateur.
    pub usage: &'static str,
    pub width: u32,
    pub height: u32,
    /// Débit vidéo visé et plafond (kb/s) pour 30 i/s ; ×1,5 au-delà de 30 i/s.
    pub video_kbps: u32,
    pub max_kbps: u32,
    pub audio_kbps: u32,
    pub max_fps: f64,
    /// Volume intégré visé (LUFS) et crête vraie maximale (dBTP).
    pub lufs: f64,
    pub peak: f64,
    /// Durée maximale acceptée (secondes), None : pas de limite gênante.
    pub max_seconds: Option<f64>,
    /// Précision sur la durée maximale (compte, application…).
    pub limit_note: &'static str,
}

const fn p(
    id: &'static str,
    label: &'static str,
    usage: &'static str,
    (width, height): (u32, u32),
    (video_kbps, max_kbps, audio_kbps): (u32, u32, u32),
    max_fps: f64,
    lufs: f64,
    max_seconds: Option<f64>,
    limit_note: &'static str,
) -> Preset {
    Preset { id, label, usage, width, height, video_kbps, max_kbps, audio_kbps, max_fps, lufs, peak: -1.0, max_seconds, limit_note }
}

/// Volume : −14 LUFS là où la plateforme ramène les vidéos plus fortes à ce niveau (YouTube,
/// TikTok, Instagram ; X, sans cible publiée, suit l'usage des réseaux sociaux) ; −16 LUFS
/// ailleurs (Facebook, LinkedIn, Vimeo : pas de normalisation publiée, recommandation AES TD1004
/// pour l'écoute en ligne). Crête vraie sous −1 dBTP partout (marge pour leur réencodage).
pub const PRESETS: [Preset; 10] = [
    p("youtube", "YouTube", "vidéo classique, en largeur", (1920, 1080), (8000, 12000, 320), 60.0, -14.0, None, ""),
    p("youtube_shorts", "YouTube Shorts", "vidéo verticale courte", (1080, 1920), (8000, 12000, 256), 60.0, -14.0, Some(180.0), "3 min au plus"),
    p("tiktok", "TikTok", "vidéo verticale", (1080, 1920), (8000, 12000, 256), 60.0, -14.0, Some(600.0), "10 min depuis l'application (60 min depuis le site)"),
    p("instagram_reels", "Instagram Reels", "vidéo verticale", (1080, 1920), (6000, 10000, 256), 60.0, -14.0, Some(180.0), "3 min au plus"),
    p("instagram_fil", "Instagram (fil, 4:5)", "le plus grand format du fil", (1080, 1350), (6000, 10000, 256), 60.0, -14.0, Some(3600.0), "60 min au plus"),
    p("instagram_carre", "Instagram (carré)", "fil et grille du profil", (1080, 1080), (5000, 8000, 256), 60.0, -14.0, Some(3600.0), "60 min au plus"),
    p("facebook", "Facebook", "vidéo du fil, en largeur", (1920, 1080), (8000, 12000, 256), 60.0, -16.0, Some(14400.0), "4 h au plus"),
    p("linkedin", "LinkedIn", "vidéo du fil, en largeur", (1920, 1080), (8000, 10000, 192), 60.0, -16.0, Some(900.0), "15 min depuis un ordinateur (10 min depuis le téléphone)"),
    p("x", "X (Twitter)", "vidéo de publication", (1920, 1080), (8000, 12000, 192), 60.0, -14.0, Some(140.0), "2 min 20 sans abonnement Premium"),
    p("vimeo", "Vimeo", "haute qualité, en largeur", (1920, 1080), (16000, 20000, 320), 60.0, -16.0, None, ""),
];

pub fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a.max(1)
    } else {
        gcd(b, a % b)
    }
}

/// Format d'une image en mots courants : 9:16, 16:9, 4:5, 1:1, 4:3… (à 1 % près).
pub fn ratio_name(w: u32, h: u32) -> String {
    if w == 0 || h == 0 {
        return "?".into();
    }
    let r = w as f64 / h as f64;
    for (a, b) in [(16, 9), (9, 16), (1, 1), (4, 5), (5, 4), (4, 3), (3, 4), (21, 9), (3, 2), (2, 3)] {
        if (r - a as f64 / b as f64).abs() / r < 0.01 {
            return format!("{a}:{b}");
        }
    }
    let g = gcd(w, h);
    format!("{}:{}", w / g, h / g)
}

/// Même format d'image (à 1 % près).
pub fn same_ratio(src: (u32, u32), dst: (u32, u32)) -> bool {
    let (a, b) = (src.0 as f64 / src.1.max(1) as f64, dst.0 as f64 / dst.1.max(1) as f64);
    (a - b).abs() / b < 0.01
}

fn even(x: f64) -> u32 {
    ((x / 2.0).round() as u32 * 2).max(2)
}

/// Taille de l'image entière posée dans la cible (dimensions paires).
pub fn fit_inside(src: (u32, u32), dst: (u32, u32)) -> (u32, u32) {
    let (sw, sh) = (src.0.max(1) as f64, src.1.max(1) as f64);
    if sw / sh > dst.0 as f64 / dst.1 as f64 {
        (dst.0, even(dst.0 as f64 * sh / sw).min(dst.1))
    } else {
        (even(dst.1 as f64 * sw / sh).min(dst.0), dst.1)
    }
}

/// Images/s de l'export : celles du montage, au plus celles de la plateforme (30 si inconnues).
pub fn out_fps(src_fps: f64, max_fps: f64) -> f64 {
    let f = if src_fps > 0.0 { src_fps } else { 30.0 };
    f.min(max_fps)
}

/// Débit vidéo (visé, plafond) en kb/s pour ces images/s.
pub fn video_rate(preset: &Preset, fps: f64) -> (u32, u32) {
    if fps > 30.5 {
        (preset.video_kbps * 3 / 2, preset.max_kbps * 3 / 2)
    } else {
        (preset.video_kbps, preset.max_kbps)
    }
}

/// Filtres ffmpeg (entrée [0:v], sortie [v]) : mise à la taille de la cible ; format différent :
/// image entière sur fond flou (la même image, couvrant la cible, réduite au 1/8, floutée et
/// assombrie, puis agrandie), sans rien couper.
pub fn video_filter(src: (u32, u32), preset: &Preset, src_fps: f64) -> String {
    let (tw, th) = (preset.width, preset.height);
    let fps = out_fps(src_fps, preset.max_fps);
    let tail = if src_fps > 0.0 && fps < src_fps { format!("fps={},format=yuv420p", util::fmt_g(fps, 6)) } else { "format=yuv420p".into() };
    if same_ratio(src, (tw, th)) {
        return format!("[0:v]scale={tw}:{th}:flags=lanczos,setsar=1,{tail}[v]");
    }
    let (fw, fh) = fit_inside(src, (tw, th));
    let (bw, bh) = (even(tw as f64 / 8.0), even(th as f64 / 8.0));
    format!(
        "[0:v]split=2[fg0][bg0];\
         [bg0]scale={bw}:{bh}:force_original_aspect_ratio=increase,crop={bw}:{bh},gblur=sigma=5,eq=brightness=-0.06:saturation=0.85,scale={tw}:{th},setsar=1[bg];\
         [fg0]scale={fw}:{fh}:flags=lanczos,setsar=1[fg];\
         [bg][fg]overlay=(W-w)/2:(H-h)/2,{tail}[v]"
    )
}

/// Comment l'image est mise au format de la plateforme, en mots d'utilisateur.
pub fn framing_note(src: (u32, u32), preset: &Preset) -> String {
    if same_ratio(src, (preset.width, preset.height)) {
        format!("même format que le montage ({}) : image entière", ratio_name(src.0, src.1))
    } else {
        format!(
            "montage en {}, plateforme en {} : image entière sur fond flou (rien n'est coupé, ni les visages)",
            ratio_name(src.0, src.1),
            ratio_name(preset.width, preset.height)
        )
    }
}

/// « 2 min 05 », « 45 s ».
pub fn fmt_duration(s: f64) -> String {
    let t = s.round() as i64;
    if t < 60 {
        format!("{t} s")
    } else if t < 3600 {
        format!("{} min {:02}", t / 60, t % 60)
    } else {
        format!("{} h {:02}", t / 3600, (t % 3600) / 60)
    }
}

/// Durée maximale : (durée à garder si la vidéo est coupée, avertissement).
pub fn duration_check(duration: f64, preset: &Preset, cut: bool) -> (Option<f64>, Option<String>) {
    let Some(max) = preset.max_seconds else { return (None, None) };
    if duration <= max + 0.05 {
        return (None, None);
    }
    if cut {
        let msg = format!("Vidéo coupée à {} : {} accepte {} ({} de montage au total).", fmt_duration(max), preset.label, preset.limit_note, fmt_duration(duration));
        (Some(max), Some(msg))
    } else {
        let msg = format!(
            "Trop longue pour {} : la vidéo dure {}, la plateforme accepte {}. Elle sera refusée ou coupée à la publication ; « Couper à la durée maximale » la raccourcit.",
            preset.label,
            fmt_duration(duration),
            preset.limit_note
        );
        (None, Some(msg))
    }
}

// --- Lecture d'une vidéo (sortie de « ffmpeg -i ») --------------------------------------------

/// Ce qu'il faut savoir d'une vidéo pour l'exporter et la contrôler.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probe {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    pub video_codec: String,
    pub video_kbps: f64,
    pub audio_codec: String,
    pub audio_kbps: f64,
    pub has_audio: bool,
}

static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|, )(\d{2,5})x(\d{2,5})(?:[ ,]|$)").unwrap());
static FPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r", ([\d.]+) fps").unwrap());
static KBPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r", (\d+) kb/s").unwrap());
static CODEC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:Video|Audio): (\w+)").unwrap());

pub fn parse_probe(info: &str) -> Probe {
    let mut p = Probe { duration: progress::duration_of(info), ..Default::default() };
    for line in info.lines() {
        let kbps = || KBPS.captures(line).and_then(|c| c[1].parse::<f64>().ok()).unwrap_or(0.0);
        let codec = || CODEC.captures(line).map(|c| c[1].to_string()).unwrap_or_default();
        if line.contains("Video:") && p.width == 0 {
            if let Some(c) = SIZE.captures(line) {
                p.width = c[1].parse().unwrap_or(0);
                p.height = c[2].parse().unwrap_or(0);
            }
            p.fps = FPS.captures(line).and_then(|c| c[1].parse().ok()).unwrap_or(0.0);
            p.video_codec = codec();
            p.video_kbps = kbps();
        } else if line.contains("Audio:") && !p.has_audio {
            p.has_audio = true;
            p.audio_codec = codec();
            p.audio_kbps = kbps();
        }
    }
    p
}

pub fn probe(path: &Path) -> Probe {
    parse_probe(&procs::ffmpeg_info(path))
}

// --- Fichiers ------------------------------------------------------------------------------------

/// Vidéo entière à exporter : la 4K si elle n'est pas plus ancienne que l'aperçu, sinon l'aperçu.
pub fn default_source() -> Option<PathBuf> {
    let (apercu, uhd) = (paths::out().join("montage_apercu.mp4"), paths::out().join("montage_4k.mp4"));
    let mt = |p: &Path| util::mtime(p).unwrap_or(0.0);
    match (apercu.exists(), uhd.exists()) {
        (true, true) if mt(&uhd) + 1.0 >= mt(&apercu) => Some(uhd),
        (_, true) if !apercu.exists() => Some(uhd),
        (true, _) => Some(apercu),
        _ => None,
    }
}

/// Nom du rush (sans dossier ni extension), pour nommer les fichiers.
pub fn rush_name(source: &Path) -> String {
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    util::read_text(paths::work().join("rush.txt")).filter(|r| !r.is_empty()).map(|r| stem(Path::new(&r))).unwrap_or_else(|| stem(source))
}

pub fn out_dir() -> PathBuf {
    paths::out().join("export")
}

pub fn summary_path() -> PathBuf {
    paths::work().join("exports.json")
}

/// Chemin relatif au projet écrit avec « / » (out/export/…), sinon le chemin entier.
fn rel(path: &Path) -> String {
    match path.strip_prefix(paths::root()) {
        Ok(r) => r.to_string_lossy().replace('\\', "/"),
        Err(_) => paths::display(path),
    }
}

fn r1(x: f64) -> Value {
    if x.is_finite() {
        util::num(util::round_to(x, 1))
    } else {
        Value::Null
    }
}

/// Description d'un préréglage pour les pages (sans source : format seul).
pub fn preset_json(preset: &Preset, src: Option<(u32, u32)>) -> Value {
    json!({
        "id": preset.id, "label": preset.label, "usage": preset.usage,
        "format": ratio_name(preset.width, preset.height), "width": preset.width, "height": preset.height,
        "videoKbps": preset.video_kbps, "audioKbps": preset.audio_kbps, "maxFps": preset.max_fps,
        "lufs": preset.lufs, "peak": preset.peak, "maxSeconds": preset.max_seconds, "limitNote": preset.limit_note,
        "blur": src.map(|s| !same_ratio(s, (preset.width, preset.height))),
        "framing": src.map(|s| framing_note(s, preset)),
    })
}

/// Exports faits (work/exports.json) dont le fichier existe encore, avec « stale » (fait depuis
/// une vidéo entière plus ancienne que celle d'aujourd'hui) et l'adresse du fichier (/out/…).
pub fn done_exports() -> Vec<Value> {
    let summary = util::read_obj(summary_path());
    let source_mt = default_source().and_then(|s| util::mtime(&s)).unwrap_or(0.0);
    let mut out = vec![];
    for preset in &PRESETS {
        let Some(Value::Object(e)) = summary.get("exports").and_then(|x| x.get(preset.id)) else { continue };
        let file = e.get("file").and_then(Value::as_str).unwrap_or("");
        let path = file.split('/').fold(paths::root().to_path_buf(), |p, x| p.join(x));
        if file.is_empty() || !path.is_file() {
            continue;
        }
        let mut e = e.clone();
        let made_from = e.get("sourceMtime").and_then(Value::as_f64).unwrap_or(0.0);
        e.insert("stale".into(), json!(source_mt > made_from + 1.0));
        if let Some(r) = file.strip_prefix("out/") {
            e.insert("url".into(), json!(format!("/out/{r}")));
            e.insert("parts".into(), json!([format!("/out/{r}")]));
        }
        e.insert("filename".into(), json!(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        out.push(Value::Object(e));
    }
    out
}

/// Liste pour /api/export : vidéo source, préréglages (cadrage d'après le format du montage) et
/// exports faits.
pub fn listing() -> Value {
    let src = default_source();
    let info = src.as_ref().map(|s| probe(s));
    let size = info.as_ref().filter(|i| i.width > 0).map(|i| (i.width, i.height));
    let source = match (&src, &info) {
        (Some(s), Some(i)) => json!({"file": rel(s), "width": i.width, "height": i.height, "format": size.map(|(w, h)| ratio_name(w, h)),
                                     "duration": r1(i.duration), "fps": i.fps}),
        _ => Value::Null,
    };
    let presets: Vec<Value> = PRESETS
        .iter()
        .map(|p| {
            let mut v = preset_json(p, size);
            if let (Some(i), Value::Object(m)) = (&info, &mut v) {
                m.insert("tooLong".into(), json!(duration_check(i.duration, p, false).1.is_some()));
            }
            v
        })
        .collect();
    json!({"source": source, "presets": presets, "exports": done_exports()})
}

// --- Export ----------------------------------------------------------------------------------

/// Son de la source amené à (cible, crête), en flottants 48 kHz stéréo dans un fichier brut
/// provisoire (*.part.* : effacé par une annulation).
fn normalized_audio(audio: &[f32], preset: &Preset, cache: &mut HashMap<String, (PathBuf, loudness::Summary)>) -> Result<(PathBuf, loudness::Summary), String> {
    let key = format!("{}_{}", preset.lufs, preset.peak);
    if let Some(v) = cache.get(&key) {
        return Ok(v.clone());
    }
    let mut data = audio.to_vec();
    let (_, after) = loudness::normalize(&mut data, 48000.0, 2, preset.lufs, preset.peak - 0.3);
    let path = out_dir().join(format!(".son{}.part.f32", cache.len()));
    let bytes: Vec<u8> = data.iter().flat_map(|x| x.to_le_bytes()).collect();
    fs::File::create(&path).and_then(|mut f| f.write_all(&bytes)).map_err(|e| format!("{} : {e}", path.display()))?;
    cache.insert(key, (path.clone(), after));
    Ok((path, after))
}

/// Arguments ffmpeg d'un export avec l'encodeur `enc` (carte graphique ou processeur : gpu.rs ;
/// sans « -progress », ajouté par progress::ffmpeg).
pub fn ffmpeg_args(src: &Path, audio: Option<&Path>, info: &Probe, preset: &Preset, keep: Option<f64>, enc: &Encoder, dst: &Path) -> Vec<String> {
    let fps = out_fps(info.fps, preset.max_fps);
    let (rate, max) = video_rate(preset, fps);
    let s = |x: &str| x.to_string();
    let mut a: Vec<String> = vec![paths::ffmpeg().to_string_lossy().into_owned(), s("-v"), s("error"), s("-y")];
    a.extend(enc.input_args());
    a.extend([s("-i"), paths::display(src)]);
    if let Some(raw) = audio {
        a.extend(["-f", "f32le", "-ar", "48000", "-ac", "2", "-i"].map(s));
        a.push(paths::display(raw));
    }
    let graph = video_filter((info.width, info.height), preset, info.fps);
    let graph = format!("{}{}[v]", graph.strip_suffix("[v]").unwrap_or(&graph), enc.filter_tail());
    a.extend([s("-filter_complex"), graph, s("-map"), s("[v]")]);
    if audio.is_some() {
        a.extend(["-map", "1:a:0", "-c:a", "aac", "-ar", "48000", "-ac", "2"].map(s));
        a.extend([s("-b:a"), format!("{}k", preset.audio_kbps)]);
    } else {
        a.push(s("-an"));
    }
    a.extend(enc.video_args(Rate::Bitrate { kbps: rate as i64, max_kbps: max as i64 }, "fast"));
    a.extend(["-profile:v", "high"].map(s));
    a.extend([s("-g"), format!("{}", (2.0 * fps).round() as i64)]);
    a.extend(["-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", "-movflags", "+faststart"].map(s));
    if let Some(t) = keep {
        a.extend([s("-t"), util::fmt_g(t, 6)]);
    } else {
        a.push(s("-shortest"));
    }
    a.push(paths::display(dst));
    a
}

struct Job<'a> {
    src: &'a Path,
    info: &'a Probe,
    audio: Option<&'a [f32]>,
    rush: &'a str,
    cut: bool,
}

/// Exporte un préréglage ; renvoie son résumé. offset / whole : avancement d'ensemble (secondes).
fn export_one(job: &Job, preset: &Preset, cache: &mut HashMap<String, (PathBuf, loudness::Summary)>, offset: f64, whole: f64) -> Result<Value, String> {
    let (keep, warning) = duration_check(job.info.duration, preset, job.cut);
    let mut warnings: Vec<String> = warning.into_iter().collect();
    let raw = match job.audio {
        Some(a) => {
            let (path, s) = normalized_audio(a, preset, cache)?;
            if !s.integrated.is_finite() {
                warnings.push("Son silencieux : volume non ajusté.".into());
            }
            Some(path)
        }
        None => {
            warnings.push("La vidéo entière n'a pas de son.".into());
            None
        }
    };
    let dst = out_dir().join(format!("{}_{}.mp4", job.rush, preset.id));
    let part = out_dir().join(format!("{}_{}.part.mp4", job.rush, preset.id));
    let total = keep.unwrap_or(job.info.duration);
    let code = gpu::encode(|enc| {
        let args = ffmpeg_args(job.src, raw.as_deref(), job.info, preset, keep, enc, &part);
        progress::ffmpeg("Export pour les plateformes", &args, Some(total), offset, Some(whole))
    });
    if code != 0 {
        let _ = fs::remove_file(&part);
        return Err(format!("{} : échec de l'encodage (ffmpeg, {})", preset.label, progress::exit_reason(code, false)));
    }
    let _ = fs::remove_file(&dst);
    fs::rename(&part, &dst).map_err(|e| e.to_string())?;
    let made = probe(&dst);
    let size = fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
    let (lufs, peak) = match loudness::measure_file(&dst) {
        Ok(m) if made.has_audio => (m.integrated, m.true_peak),
        _ => (f64::NAN, f64::NAN),
    };
    if lufs.is_finite() && (lufs - preset.lufs).abs() > 1.0 {
        warnings.push(format!("Volume mesuré {} au lieu de {:.0} LUFS.", loudness::fmt_lufs(lufs), preset.lufs));
    }
    let src_size = (job.info.width, job.info.height);
    Ok(json!({
        "preset": preset.id, "label": preset.label, "file": rel(&dst), "size": size,
        "duration": r1(made.duration), "width": made.width, "height": made.height, "fps": made.fps,
        "format": ratio_name(made.width, made.height), "videoCodec": made.video_codec, "audioCodec": made.audio_codec,
        "videoKbps": made.video_kbps, "audioKbps": made.audio_kbps,
        "lufs": r1(lufs), "peak": r1(peak), "target": preset.lufs, "peakMax": preset.peak,
        "blur": !same_ratio(src_size, (preset.width, preset.height)), "framing": framing_note(src_size, preset),
        "cut": keep.is_some(), "warnings": warnings,
        "source": rel(job.src), "sourceMtime": util::mtime(job.src).unwrap_or(0.0),
        "at": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
    }))
}

/// Exporte les préréglages demandés depuis `src` ; résumé ajouté à work/exports.json.
/// Renvoie le nombre d'échecs.
pub fn run(ids: &[&'static Preset], src: &Path, cut: bool) -> Result<usize, String> {
    let info = probe(src);
    if info.width == 0 || info.duration <= 0.0 {
        return Err(format!("{} : vidéo illisible", src.display()));
    }
    util::mkdirs(out_dir());
    let rush = rush_name(src);
    say(&format!(
        "Source : {} ({}x{}, {}, {:.1} i/s, {}), {} export(s)",
        rel(src),
        info.width,
        info.height,
        ratio_name(info.width, info.height),
        info.fps,
        fmt_duration(info.duration),
        ids.len()
    ));
    let audio = if info.has_audio {
        say("Mesure du son de la vidéo entière…");
        let a = loudness::decode(src)?;
        let m = loudness::measure(&a, 48000.0, 2);
        say(&format!("Son de la vidéo entière : {}, crête {:.1} dBTP", loudness::fmt_lufs(m.integrated), m.true_peak));
        Some(a)
    } else {
        None
    };
    let lengths: Vec<f64> = ids.iter().map(|p| duration_check(info.duration, p, cut).0.unwrap_or(info.duration)).collect();
    let whole: f64 = lengths.iter().sum();
    let job = Job { src, info: &info, audio: audio.as_deref(), rush: &rush, cut };
    let mut cache = HashMap::new();
    let (mut offset, mut failed) = (0.0, 0);
    let path = summary_path();
    let start_mt = util::mtime(src);
    for (k, preset) in ids.iter().enumerate() {
        // vidéo entière refaite pendant l'export (génération lancée ailleurs) : son déjà lu périmé
        if util::mtime(src) != start_mt {
            say(&format!("ERREUR : {} a changé pendant l'export : relancez-le.", rel(src)));
            failed += ids.len() - k;
            break;
        }
        say(&format!("\n{} ({}/{}) : {}x{} {}, {}", preset.label, k + 1, ids.len(), preset.width, preset.height, ratio_name(preset.width, preset.height), framing_note((info.width, info.height), preset)));
        match export_one(&job, preset, &mut cache, offset, whole) {
            Ok(entry) => {
                say(&format!(
                    "  {} : {}x{}, {:.1} Mo, {:.0} kb/s, {}, crête {} dBTP",
                    entry["file"].as_str().unwrap_or(""),
                    entry["width"],
                    entry["height"],
                    entry["size"].as_f64().unwrap_or(0.0) / 1048576.0,
                    entry["videoKbps"].as_f64().unwrap_or(0.0),
                    loudness::fmt_lufs(entry["lufs"].as_f64().unwrap_or(f64::NAN)),
                    entry["peak"]
                ));
                for w in util::arr(&entry, "warnings") {
                    say(&format!("  Attention : {}", w.as_str().unwrap_or("")));
                }
                // résumé écrit après chaque export : un export annulé garde les précédents
                let mut all = util::read_obj(&path);
                let mut exports = all.get("exports").and_then(Value::as_object).cloned().unwrap_or_default();
                exports.insert(preset.id.into(), entry);
                all.insert("exports".into(), Value::Object(exports));
                util::mkdirs(paths::work());
                if let Err(e) = util::write_json_atomic(&path, &Value::Object(all), Some(1)) {
                    say(&format!("Attention : résumé {} non écrit ({e})", path.display()));
                }
            }
            Err(e) => {
                say(&format!("ERREUR : {e}"));
                failed += 1;
            }
        }
        offset += lengths[k];
    }
    for (raw, _) in cache.values() {
        let _ = fs::remove_file(raw);
    }
    Ok(failed)
}

fn print_list(src: Option<(u32, u32)>) {
    println!("{:16} {:22} {:>9} {:>6} {:>10} {:>8} {:>6} {:>10}", "préréglage", "plateforme", "taille", "format", "vidéo", "son", "LUFS", "durée max");
    for p in &PRESETS {
        println!(
            "{:16} {:22} {:>9} {:>6} {:>7} kb/s {:>3} kb/s {:>6} {:>10}",
            p.id,
            p.label,
            format!("{}x{}", p.width, p.height),
            ratio_name(p.width, p.height),
            p.video_kbps,
            p.audio_kbps,
            p.lufs,
            p.max_seconds.map(fmt_duration).unwrap_or_else(|| "-".into())
        );
        if let Some(s) = src {
            println!("{:16} {}", "", framing_note(s, p));
        }
    }
}

const USAGE: &str = "Usage : auto-montage export <préréglage…>|tout [--couper] [--source <fichier>]
        auto-montage export --liste [--json]
  --couper  coupe à la durée maximale de la plateforme (sinon : avertissement)
  --source  vidéo à exporter (par défaut out/montage_4k.mp4 si à jour, sinon out/montage_apercu.mp4)";

pub fn main(args: &[String]) -> i32 {
    let mut source: Option<PathBuf> = None;
    let (mut cut, mut list, mut as_json) = (false, false, false);
    let mut ids: Vec<&'static Preset> = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--couper" => cut = true,
            "--liste" | "--list" => list = true,
            "--json" => as_json = true,
            "--source" => match it.next() {
                Some(s) => source = Some(PathBuf::from(s)),
                None => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            },
            "tout" | "all" => ids.extend(PRESETS.iter()),
            id => match find(id) {
                Some(p) if !ids.contains(&p) => ids.push(p),
                Some(_) => {}
                None => {
                    eprintln!("Préréglage inconnu : {id} (liste : auto-montage export --liste)");
                    return 2;
                }
            },
        }
    }
    if list {
        if as_json {
            println!("{}", crate::pyjson::dumps_utf8(&listing()));
        } else {
            let src = source.or_else(default_source).map(|s| probe(&s)).filter(|i| i.width > 0).map(|i| (i.width, i.height));
            print_list(src);
        }
        return 0;
    }
    if ids.is_empty() {
        eprintln!("{USAGE}");
        return 2;
    }
    let Some(src) = source.or_else(default_source) else {
        eprintln!("Pas de vidéo entière rendue (out/montage_apercu.mp4) : générez-la d'abord.");
        return 1;
    };
    match run(&ids, &src, cut) {
        Ok(0) => {
            say(&format!("\nExports prêts dans {}/ (résumé : work/exports.json)", rel(&out_dir())));
            0
        }
        Ok(n) => {
            say(&format!("\n{n} export(s) en échec sur {}", ids.len()));
            1
        }
        Err(e) => {
            eprintln!("Erreur : {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid_and_unique() {
        let mut ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len(), "identifiants en double");
        for p in &PRESETS {
            assert!(p.width % 2 == 0 && p.height % 2 == 0, "{} : dimensions paires", p.id);
            assert!(p.max_kbps >= p.video_kbps && p.video_kbps > 0, "{}", p.id);
            assert!((-24.0..=-9.0).contains(&p.lufs) && p.peak <= -1.0, "{}", p.id);
            assert!(p.max_seconds.is_none() || !p.limit_note.is_empty(), "{} : durée maximale sans explication", p.id);
            assert!(p.id.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{} : identifiant de fichier", p.id);
        }
        // formats couverts : largeur, vertical, carré, 4:5
        let formats: Vec<String> = PRESETS.iter().map(|p| ratio_name(p.width, p.height)).collect();
        for f in ["16:9", "9:16", "1:1", "4:5"] {
            assert!(formats.iter().any(|x| x == f), "format {f} absent");
        }
        assert_eq!(find("youtube").unwrap().lufs, -14.0);
        assert_eq!(find("vimeo").unwrap().lufs, -16.0);
        assert_eq!(find("youtube_shorts").unwrap().max_seconds, Some(180.0));
        assert!(find("inconnu").is_none());
    }

    #[test]
    fn ratios() {
        assert_eq!(ratio_name(1080, 1920), "9:16");
        assert_eq!(ratio_name(3840, 2160), "16:9");
        assert_eq!(ratio_name(1080, 1350), "4:5");
        assert_eq!(ratio_name(1440, 1080), "4:3");
        assert_eq!(ratio_name(1080, 1080), "1:1");
        assert_eq!(ratio_name(1000, 300), "10:3");
        assert!(same_ratio((3840, 2160), (1920, 1080)));
        assert!(!same_ratio((1920, 1080), (1080, 1920)));
    }

    #[test]
    fn image_entiere_dans_la_cible() {
        // montage en largeur vers une cible verticale : pleine largeur, bandes floues en haut et en bas
        assert_eq!(fit_inside((1920, 1080), (1080, 1920)), (1080, 608));
        // vertical vers largeur : pleine hauteur
        assert_eq!(fit_inside((1080, 1920), (1920, 1080)), (608, 1080));
        // 16:9 vers 4:5 et carré
        assert_eq!(fit_inside((1920, 1080), (1080, 1350)), (1080, 608));
        assert_eq!(fit_inside((1080, 1920), (1080, 1080)), (608, 1080));
        // 4:3 vers 16:9 : bandes sur les côtés
        assert_eq!(fit_inside((1440, 1080), (1920, 1080)), (1440, 1080));
    }

    #[test]
    fn filtres() {
        let yt = find("youtube").unwrap();
        // même format : simple mise à la taille, images/s gardées
        let f = video_filter((3840, 2160), yt, 30.0);
        assert_eq!(f, "[0:v]scale=1920:1080:flags=lanczos,setsar=1,format=yuv420p[v]");
        // autre format : fond flou, image entière centrée, pas de recadrage de l'image
        let f = video_filter((1080, 1920), yt, 30.0);
        assert!(f.contains("split=2") && f.contains("gblur") && f.contains("overlay=(W-w)/2:(H-h)/2"), "{f}");
        assert!(f.contains("[fg0]scale=608:1080:"), "{f}");
        assert!(f.contains("crop=240:136"), "{f}"); // seul le fond est recadré
        assert!(framing_note((1080, 1920), yt).contains("fond flou"));
        assert!(framing_note((1920, 1080), yt).contains("même format"));
        // images/s plafonnées
        let mut slow = *yt;
        slow.max_fps = 30.0;
        assert!(video_filter((1920, 1080), &slow, 60.0).contains("fps=30,"));
        assert_eq!(out_fps(0.0, 60.0), 30.0);
        assert_eq!(video_rate(yt, 60.0), (12000, 18000));
        assert_eq!(video_rate(yt, 30.0), (8000, 12000));
    }

    #[test]
    fn duree_maximale() {
        let x = find("x").unwrap();
        assert_eq!(duration_check(120.0, x, false), (None, None));
        let (keep, w) = duration_check(200.0, x, false);
        assert_eq!(keep, None);
        let w = w.unwrap();
        assert!(w.contains("Trop longue pour X") && w.contains("3 min 20") && w.contains("Couper"), "{w}");
        let (keep, w) = duration_check(200.0, x, true);
        assert_eq!(keep, Some(140.0));
        assert!(w.unwrap().contains("coupée à 2 min 20"));
        assert_eq!(duration_check(99999.0, find("youtube").unwrap(), false), (None, None));
        assert_eq!(fmt_duration(45.2), "45 s");
        assert_eq!(fmt_duration(3725.0), "1 h 02");
    }

    #[test]
    fn lecture_de_ffmpeg() {
        let info = "  Duration: 00:01:56.63, start: 0.000000, bitrate: 5294 kb/s
  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 5095 kb/s, 30 fps, 30 tbr, 15360 tbn (default)
  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 191 kb/s (default)";
        let p = parse_probe(info);
        assert_eq!((p.width, p.height, p.fps), (1920, 1080, 30.0));
        assert!((p.duration - 116.63).abs() < 1e-6);
        assert_eq!((p.video_codec.as_str(), p.video_kbps), ("h264", 5095.0));
        assert_eq!((p.audio_codec.as_str(), p.audio_kbps, p.has_audio), ("aac", 191.0, true));
        assert!(!parse_probe("rien").has_audio);
    }

    #[test]
    fn arguments_ffmpeg() {
        let info = Probe { width: 1920, height: 1080, fps: 30.0, duration: 200.0, has_audio: true, ..Default::default() };
        let a = ffmpeg_args(Path::new("in.mp4"), Some(Path::new("son.f32")), &info, find("x").unwrap(), Some(140.0), &Encoder::cpu(), Path::new("o.mp4"));
        let s = a.join(" ");
        for want in ["-c:v libx264", "-c:a aac", "-b:a 192k", "-b:v 8000k", "-maxrate 12000k", "-movflags +faststart", "-t 140", "-g 60", "-map 1:a:0"] {
            assert!(s.contains(want), "{want} absent : {s}");
        }
        let a = ffmpeg_args(Path::new("in.mp4"), None, &info, find("youtube").unwrap(), None, &Encoder::cpu(), Path::new("o.mp4"));
        assert!(a.contains(&"-an".to_string()) && a.contains(&"-shortest".to_string()));
    }
}
