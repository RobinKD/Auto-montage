//! Outils communs du serveur local : dossiers, version, enveloppe des pages (SKELETON), chemins
//! sûrs (`inside`), conversions à la façon de Python (`float()`, `int()`, dates ISO…).

use crate::moments_lib::{app_mode, derush_subtitles};
use crate::{paths, pyjson, util};
use regex::Regex;
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

pub fn root() -> &'static Path {
    paths::root()
}
pub fn work() -> PathBuf {
    paths::work()
}
pub fn moments() -> PathBuf {
    work().join("moments")
}
pub fn db() -> PathBuf {
    work().join("local_db")
}
pub fn out() -> PathBuf {
    paths::out()
}
pub fn rushes() -> PathBuf {
    paths::rushes()
}
pub fn logs() -> PathBuf {
    work().join("logs")
}
pub fn edit_json() -> PathBuf {
    paths::data().join("edit.json")
}

pub use crate::place::{is_video, DERIVED};

/// Menu : bandeau des pages.
pub const SHIM: &str = "<script src=\"/local/shim.js\"></script>\n<script src=\"/local/menu.js\"></script>\n";

fn version_file() -> PathBuf {
    root().join("..").join("VERSION")
}

/// Version installée (fichier VERSION à la racine du dépôt), lue au démarrage : les lanceurs la
/// comparent à celle des fichiers pour redémarrer le serveur après une mise à jour.
pub static VERSION: LazyLock<String> = LazyLock::new(|| {
    let f = version_file();
    if f.exists() {
        fs::read_to_string(&f).map(|s| s.trim().to_string()).unwrap_or_else(|_| "dev".into())
    } else {
        "dev".into()
    }
});

/// Démarrage du serveur : la page voit qu'il a redémarré (même version).
pub static STARTED: LazyLock<f64> = LazyLock::new(util::now_secs);

/// Version des fichiers maintenant (différente de VERSION si le projet a changé sur le disque
/// depuis le démarrage, ex. git pull dans le dossier d'installation : redémarrage à faire).
pub fn disk_version() -> String {
    fs::read_to_string(version_file()).map(|s| s.trim().to_string()).unwrap_or_else(|_| VERSION.clone())
}

/// Même enveloppe que celle que claude.ai ajoute aux pages publiées.
pub static SKELETON: LazyLock<String> = LazyLock::new(|| {
    format!(
        "<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1,viewport-fit=cover\">\
         <style>body{{margin:0}}img{{max-width:100%}}[hidden]{{display:none!important}}</style>\
         <script>window.AUTO_MONTAGE_VERSION = {};</script>{}</head><body>",
        pyjson::dumps(&Value::String(VERSION.clone())),
        SHIM
    )
});

/// Mode « Dérushage » : les parties marquées data-mode-montage (effets, sous-titres, Claude) sont
/// masquées dès le chargement ; local/menu.js montre le choix du mode dans le bandeau.
pub const SKELETON_STYLE: &str = "html[data-mode=\"derush\"] [data-mode-montage],\
html[data-mode=\"derush\"][data-subtitles=\"0\"] [data-subtitles-only]{display:none!important}";

pub fn with_mode(skeleton: &str) -> String {
    let mode = app_mode(&work());
    let subs = if derush_subtitles(&work()) { "1" } else { "0" };
    skeleton
        .replacen("<html lang=\"fr\">", &format!("<html lang=\"fr\" data-mode=\"{mode}\" data-subtitles=\"{subs}\">"), 1)
        .replacen("</style>", &format!("{SKELETON_STYLE}</style>"), 1)
}

/// Fonction réservée au mode Auto-montage (Claude, effets) : message d'erreur, sinon None.
pub fn derush_refused() -> Option<&'static str> {
    (app_mode(&work()) == "derush")
        .then_some("Indisponible en mode Dérushage : passez en mode Auto-montage dans le bandeau du haut.")
}

/// Identifiant sûr (collection, document, montage…) : ni « . » ni « .. », caractères simples.
pub fn safe_id(s: &str) -> bool {
    let n = s.chars().count();
    !s.starts_with('.')
        && (1..=200).contains(&n)
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "_.~:@+-".contains(c))
}

/// `os.path.realpath` : liens symboliques résolus pour ce qui existe, le reste normalisé.
pub fn realpath(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for comp in abs.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            Component::Normal(c) => {
                out.push(c);
                if let Ok(r) = fs::canonicalize(&out) {
                    out = r;
                }
            }
            c => out.push(c.as_os_str()), // préfixe (C:) et racine : tels quels
        }
    }
    out
}

/// Chemin de rel sous base, ou None s'il en sort.
pub fn inside(base: &Path, rel: &str) -> Option<PathBuf> {
    let path = realpath(&base.join(rel));
    let base = realpath(base);
    (path == base || path.starts_with(&base)).then_some(path)
}

/// Texte d'un fichier sans espaces autour, ou None.
pub fn read_text(path: impl AsRef<Path>) -> Option<String> {
    util::read_text(path)
}

/// `os.path.basename` (séparateur « / », et « \ » aussi sous Windows).
pub fn basename(s: &str) -> &str {
    let cut = if cfg!(windows) { s.rfind(['/', '\\']) } else { s.rfind('/') };
    match cut {
        Some(i) => &s[i + 1..],
        None => s,
    }
}

/// `os.path.splitext` : (racine, extension avec son point).
pub fn splitext(p: &str) -> (&str, &str) {
    let sep = if cfg!(windows) { p.rfind(['/', '\\']) } else { p.rfind('/') };
    let start = sep.map(|i| i + 1).unwrap_or(0);
    if let Some(dot) = p.rfind('.') {
        if dot > start && p[start..dot].chars().any(|c| c != '.') {
            return (&p[..dot], &p[dot..]);
        }
    }
    (p, "")
}

/// `html.escape` (guillemets compris).
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

/// `str.capitalize()` : première lettre en majuscule, le reste en minuscules.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// `float(x)` de Python : None si la conversion lèverait une erreur.
pub fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => {
            let t = s.trim().replace('_', "");
            match t.to_lowercase().as_str() {
                "inf" | "+inf" | "infinity" | "+infinity" => Some(f64::INFINITY),
                "-inf" | "-infinity" => Some(f64::NEG_INFINITY),
                "nan" | "+nan" | "-nan" => Some(f64::NAN),
                _ => t.parse().ok(),
            }
        }
        _ => None,
    }
}

/// `int(x)` de Python : flottant tronqué, texte d'un entier seulement.
pub fn py_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().filter(|f| f.is_finite()).map(|f| f.trunc() as i64)),
        Value::Bool(b) => Some(*b as i64),
        Value::String(s) => s.trim().replace('_', "").parse().ok(),
        _ => None,
    }
}

fn local_dt(ts: f64) -> chrono::DateTime<chrono::Local> {
    let secs = ts.floor();
    let mut micros = ((ts - secs) * 1e6).round() as i64;
    let mut secs = secs as i64;
    if micros >= 1_000_000 {
        secs += 1;
        micros -= 1_000_000;
    }
    chrono::DateTime::from_timestamp(secs, (micros * 1000) as u32)
        .unwrap_or_default()
        .with_timezone(&chrono::Local)
}

fn micro_suffix(nanos: u32) -> String {
    let us = nanos / 1000;
    if us == 0 {
        String::new()
    } else {
        format!(".{us:06}")
    }
}

/// `datetime.fromtimestamp(ts).isoformat()` (microsecondes s'il y en a).
pub fn iso_local(ts: f64) -> String {
    let d = local_dt(ts);
    format!("{}{}", d.format("%Y-%m-%dT%H:%M:%S"), micro_suffix(chrono::Timelike::nanosecond(&d)))
}

/// `datetime.fromtimestamp(ts).isoformat(timespec="seconds")`.
pub fn iso_local_secs(ts: f64) -> String {
    local_dt(ts).format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// `datetime.fromtimestamp(ts, timezone.utc).isoformat()`.
pub fn iso_utc(ts: f64) -> String {
    let d = local_dt(ts).with_timezone(&chrono::Utc);
    format!("{}{}+00:00", d.format("%Y-%m-%dT%H:%M:%S"), micro_suffix(chrono::Timelike::nanosecond(&d)))
}

/// `datetime.datetime.now().isoformat()`.
pub fn now_iso() -> String {
    iso_local(util::now_secs())
}

/// `datetime.datetime.now().isoformat(timespec="seconds")`.
pub fn now_iso_secs() -> String {
    iso_local_secs(util::now_secs())
}

/// Date locale formatée (strftime).
pub fn local_fmt(ts: f64, fmt: &str) -> String {
    local_dt(ts).format(fmt).to_string()
}

pub fn now_fmt(fmt: &str) -> String {
    chrono::Local::now().format(fmt).to_string()
}

/// JSON écrit comme `json.dump(v, f, ensure_ascii=…, indent=…)`.
pub fn write_json(path: impl AsRef<Path>, v: &Value, indent: Option<usize>, ensure_ascii: bool) -> std::io::Result<()> {
    let text = pyjson::dumps_with(v, pyjson::Opts { indent, ensure_ascii, sort_keys: false });
    fs::write(path, text)
}

/// Taille d'un dossier (`os.walk` + `lstat`).
pub fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .min_depth(1)
        .into_iter()
        .flatten()
        .filter(|e| !e.file_type().is_dir())
        .filter_map(|e| fs::symlink_metadata(e.path()).ok())
        .map(|m| m.len())
        .sum()
}

/// Durée d'une vidéo (ffmpeg -i), 0 si inconnue.
pub fn media_seconds(path: &Path) -> f64 {
    crate::progress::duration_of(&crate::procs::ffmpeg_info(path))
}

/// Place libre sur le disque qui contient path.
pub fn disk_free(path: &Path) -> u64 {
    crate::memoire::disk_free(path).unwrap_or(0)
}

/// Type d'un fichier servi : celui de `mimetypes` de Python pour les fichiers du projet.
pub fn mime_type(path: &Path) -> String {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let known = match ext.as_str() {
        "js" | "mjs" => Some("text/javascript"),
        "css" => Some("text/css"),
        "html" | "htm" => Some("text/html"),
        "json" => Some("application/json"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "svg" => Some("image/svg+xml"),
        "ico" => Some("image/vnd.microsoft.icon"),
        "mp4" | "m4v" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mov" => Some("video/quicktime"),
        "mkv" => Some("video/x-matroska"),
        "avi" => Some("video/x-msvideo"),
        "mts" => Some("model/vnd.mts"),
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/x-wav"),
        "ogg" | "opus" | "oga" => Some("audio/ogg"),
        "m4a" => Some("audio/mp4"),
        "flac" => Some("audio/flac"),
        "aac" => Some("audio/aac"),
        "aif" | "aiff" => Some("audio/x-aiff"),
        "ttf" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        "txt" | "srt" => Some("text/plain"),
        "md" => Some("text/markdown"),
        "vtt" => Some("text/vtt"),
        "pdf" => Some("application/pdf"),
        "zip" => Some("application/zip"),
        "xml" => Some("application/xml"),
        "wasm" => Some("application/wasm"),
        "log" | "map" | "" => None,
        _ => return mime_guess::from_path(path).first_raw().unwrap_or("application/octet-stream").to_string(),
    };
    known.unwrap_or("application/octet-stream").to_string()
}

/// `urllib.parse.unquote` (UTF-8, caractères invalides remplacés).
pub fn unquote(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()
}

/// `urllib.parse.parse_qs` : (clé, valeur) dans l'ordre, valeurs vides ignorées.
pub fn parse_qs(query: &str) -> Vec<(String, String)> {
    query
        .split(['&', ';'])
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            let k = unquote(&k.replace('+', " "));
            let v = unquote(&v.replace('+', " "));
            (!v.is_empty()).then_some((k, v))
        })
        .collect()
}

/// Nom de fichier de rush acceptable (pas de chemin, extension vidéo), ou None.
pub fn safe_rush_name(name: &str) -> Option<String> {
    static OK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w .()+,'&-]{1,200}$").unwrap());
    let name = basename(name).trim();
    if name.is_empty() || name.starts_with('.') || name.starts_with('-') || DERIVED.is_match(name) || !is_video(name) {
        return None;
    }
    OK.is_match(name).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_like_python() {
        assert_eq!(splitext("a/b.c.mov"), ("a/b.c", ".mov"));
        assert_eq!(splitext(".bashrc"), (".bashrc", ""));
        assert_eq!(splitext("rush"), ("rush", ""));
        assert_eq!(capitalize("bIP essai"), "Bip essai");
        assert!(safe_id("perso-bip"));
        assert!(!safe_id(".."));
        assert!(!safe_id("a/b"));
        assert_eq!(safe_rush_name("../x/essai.mov").as_deref(), Some("essai.mov"));
        assert_eq!(safe_rush_name("rush_1080.mp4"), None);
        assert_eq!(safe_rush_name("-x.mov"), None);
        assert_eq!(html_escape("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&#x27;");
        assert_eq!(parse_qs("rush=a+b.mov&rush=c%20d.mov&x="), vec![("rush".into(), "a b.mov".into()), ("rush".into(), "c d.mov".into())]);
        assert_eq!(py_int(&serde_json::json!(3.7)), Some(3));
        assert_eq!(py_int(&serde_json::json!("2.5")), None);
        assert_eq!(py_float(&serde_json::json!("1.5")), Some(1.5));
    }

    #[test]
    fn inside_stays_inside() {
        let base = std::env::temp_dir();
        assert!(inside(&base, "../etc/passwd").is_none());
        assert!(inside(&base, "x/../y").is_some());
        assert_eq!(inside(&base, ""), Some(realpath(&base)));
    }
}
