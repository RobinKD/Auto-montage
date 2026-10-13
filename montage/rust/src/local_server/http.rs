//! Serveur HTTP : contrôle de l'adresse et de l'origine (parse_request), fichiers statiques
//! avec requêtes partielles (Range), envois de fichiers, et toutes les routes des pages.

use super::claude;
use super::common::*;
use super::info::*;
use super::jobs::{self, busy, busy_error, cancel_job, job_status, paused_task, resume_job, start_job, Step, JOB_LOCK};
use crate::moments_lib::{app_mode, derush_subtitles, language_label, MODES};
use crate::{decoupage, fonts_lib, memoire, paths, place, procs, pyjson, util};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Noms sous lesquels l'interface est ouverte (navigateur de la machine, lanceurs) ; d'autres
/// avec AM_ALLOWED_HOSTS="nom1,nom2".
static ALLOWED_HOSTS: LazyLock<Vec<String>> = LazyLock::new(|| {
    let mut v: Vec<String> = ["localhost", "127.0.0.1", "[::1]"].iter().map(|s| s.to_string()).collect();
    let extra = std::env::var("AM_ALLOWED_HOSTS").unwrap_or_default().to_lowercase().replace(' ', "");
    v.extend(extra.split(',').filter(|s| !s.is_empty()).map(String::from));
    v
});
const JSON_MAX: u64 = 5 << 20; // corps JSON des requêtes : 5 Mo au plus

pub enum Body {
    Bytes(Vec<u8>),
    File(fs::File, u64),
    Empty,
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

/// Requête reçue : méthode, chemin (brut et décodé), paramètres, en-têtes.
pub struct Req {
    pub method: String,
    pub raw_path: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_lowercase();
        self.headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }
}

fn reason(code: u16) -> (&'static str, &'static str) {
    match code {
        400 => ("Bad Request", "Bad request syntax or unsupported method"),
        403 => ("Forbidden", "Request forbidden -- authorization will not help"),
        404 => ("Not Found", "Nothing matches the given URI"),
        411 => ("Length Required", "Client must specify Content-Length"),
        416 => ("Range Not Satisfiable", "Cannot satisfy request range"),
        500 => ("Internal Server Error", "Server got itself in trouble"),
        501 => ("Not Implemented", "Server does not support this operation"),
        _ => ("Error", ""),
    }
}

/// `send_error` de Python : petite page HTML d'erreur.
pub fn error(code: u16, message: Option<&str>) -> Reply {
    let (short, explain) = reason(code);
    let msg = message.unwrap_or(short).replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let body = format!(
        "<!DOCTYPE HTML>\n<html lang=\"en\">\n    <head>\n        <meta charset=\"utf-8\">\n        <title>Error response</title>\n    </head>\n    <body>\n        <h1>Error response</h1>\n        <p>Error code: {code}</p>\n        <p>Message: {msg}.</p>\n        <p>Error code explanation: {code} - {explain}.</p>\n    </body>\n</html>\n"
    );
    Reply {
        status: code,
        headers: vec![("Content-Type".into(), "text/html;charset=utf-8".into()), ("Connection".into(), "close".into())],
        body: Body::Bytes(body.into_bytes()),
    }
}

pub fn send_json(data: &Value, status: u16) -> Reply {
    Reply {
        status,
        headers: vec![("Content-Type".into(), "application/json; charset=utf-8".into()), ("Cache-Control".into(), "no-store".into())],
        body: Body::Bytes(pyjson::dumps_utf8(data).into_bytes()),
    }
}

fn ok_json(status: u16) -> Reply {
    send_json(&json!({"ok": true}), status)
}

fn err_json(msg: &str, status: u16) -> Reply {
    send_json(&json!({"error": msg}), status)
}

fn send_html(html: String) -> Reply {
    // mode choisi dans le bandeau : sur <html>, avant tout affichage
    let html = match html.strip_prefix(SKELETON.as_str()) {
        Some(rest) => with_mode(&SKELETON) + rest,
        None => html,
    };
    Reply {
        status: 200,
        headers: vec![("Content-Type".into(), "text/html; charset=utf-8".into()), ("Cache-Control".into(), "no-store".into())],
        body: Body::Bytes(html.into_bytes()),
    }
}

fn page(rel: &[&str]) -> Reply {
    let mut p = root().to_path_buf();
    for r in rel {
        p = p.join(r);
    }
    match fs::read_to_string(&p) {
        Ok(text) => send_html(format!("{}{text}", *SKELETON)),
        Err(_) => error(500, None),
    }
}

fn redirect(location: &str) -> Reply {
    Reply { status: 302, headers: vec![("Location".into(), location.into()), ("Content-Length".into(), "0".into())], body: Body::Empty }
}

fn http_date(ts: f64) -> String {
    chrono::DateTime::from_timestamp(ts.floor() as i64, 0).unwrap_or_default().format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// Fichier statique, avec les requêtes partielles (Range) pour avancer dans les vidéos.
fn send_file(req: &Req, path: Option<PathBuf>) -> Reply {
    let Some(path) = path.filter(|p| p.is_file()) else { return error(404, None) };
    let Ok(meta) = fs::metadata(&path) else { return error(404, None) };
    let size = meta.len() as i64;
    // Validateurs : le navigateur revalide sa copie et reçoit le nouveau fichier s'il a changé.
    let etag = format!("\"{:x}-{:x}\"", util::mtime_ns(&path).unwrap_or(0), size);
    let modified = http_date(util::mtime(&path).unwrap_or(0.0));
    if req.header("If-None-Match") == Some(etag.as_str()) {
        return Reply { status: 304, headers: vec![("ETag".into(), etag)], body: Body::Empty };
    }
    static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^bytes=(\d*)-(\d*)$").unwrap());
    let (mut start, mut end) = (0i64, size - 1);
    let mut m = RANGE.captures(req.header("Range").unwrap_or(""));
    if let Some(ir) = req.header("If-Range").filter(|s| !s.is_empty()) {
        if ir != etag && ir != modified {
            m = None; // morceau d'une ancienne version : fichier entier
        }
    }
    let mut status = 200;
    let mut headers = vec![];
    if let Some(c) = m.filter(|c| !c[1].is_empty() || !c[2].is_empty()) {
        let big = |s: &str| s.parse::<i64>().unwrap_or(i64::MAX);
        if !c[1].is_empty() {
            start = big(&c[1]);
            end = if c[2].is_empty() { size - 1 } else { big(&c[2]).min(size - 1) };
        } else {
            start = (size - big(&c[2])).max(0);
        }
        if start > end {
            return Reply { status: 416, headers: vec![("Content-Range".into(), format!("bytes */{size}"))], body: Body::Empty };
        }
        status = 206;
        headers.push(("Content-Range".into(), format!("bytes {start}-{end}/{size}")));
    }
    headers.push(("Content-Type".into(), mime_type(&path)));
    headers.push(("Accept-Ranges".into(), "bytes".into()));
    headers.push(("ETag".into(), etag));
    headers.push(("Last-Modified".into(), modified));
    headers.push(("Cache-Control".into(), "no-cache".into()));
    let Ok(mut f) = fs::File::open(&path) else { return error(404, None) };
    use std::io::Seek;
    let _ = f.seek(std::io::SeekFrom::Start(start as u64));
    Reply { status, headers, body: Body::File(f, (end - start + 1).max(0) as u64) }
}

fn py_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => (if *b { "True" } else { "False" }).into(),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => pyjson::float_repr(n.as_f64().unwrap_or(0.0)),
        },
        Some(other) => pyjson::dumps(other),
    }
}

/// `str(body.get(k) or "")`
fn body_str(body: &Map<String, Value>, k: &str) -> String {
    let v = body.get(k);
    if util::truthy(v) {
        py_str(v)
    } else {
        String::new()
    }
}

// --- GET --------------------------------------------------------------------------------
fn do_get(req: &Req) -> Reply {
    let path = req.path.as_str();
    match path {
        "/" | "/index.html" => {
            if !moments().join("index.html").exists() {
                return redirect("/rush/"); // pas encore de rush préparé
            }
            return match moments_page() {
                Ok(html) => send_html(format!("{}{}", *SKELETON, local_links(html))),
                Err(e) => {
                    eprintln!("Page des moments : {e}");
                    error(500, None)
                }
            };
        }
        "/downloads" | "/downloads/" => {
            let Ok(tpl) = fs::read_to_string(root().join("selection").join("downloads.html")) else { return error(500, None) };
            let data = downloads_data();
            let rush = data["rush"].as_str().unwrap_or("").to_string();
            let html = tpl
                .replace("__RUSH__", &html_escape(&rush))
                .replace("/*__DATA__*/null", &pyjson::dumps_utf8(&data).replace('<', "\\u003c"));
            return send_html(format!("{}{html}", *SKELETON));
        }
        "/chat" | "/chat/" => return page(&["local", "chat.html"]),
        "/api/chat" => return send_json(&claude::chat_status(), 200),
        "/rush" | "/rush/" => return page(&["local", "rush.html"]),
        "/api/version" => return send_json(&json!({"version": *VERSION, "started": util::num(*STARTED)}), 200),
        "/api/instructions" => {
            let p = instructions_path();
            let text = if p.exists() { String::from_utf8_lossy(&fs::read(&p).unwrap_or_default()).into_owned() } else { String::new() };
            return send_json(&json!({"text": text, "info": instructions_info()}), 200);
        }
        "/updates" | "/updates/" => return page(&["local", "updates.html"]),
        "/api/update" => {
            if claude::UPDATE.lock().map(|u| u.1.is_none()).unwrap_or(true) {
                claude::update_check();
            }
            let mut s = claude::update_state();
            s["version"] = json!(*VERSION);
            s["onDisk"] = json!(disk_version());
            return send_json(&s, 200);
        }
        "/claude" | "/claude/" => return page(&["local", "claude.html"]),
        "/api/claude" => {
            let connected = claude::claude_connected(true);
            return send_json(
                &json!({"connected": connected, "installed": paths::claude().is_some(),
                        "account": if connected { claude::claude_account() } else { String::new() },
                        "apiKey": std::env::var("ANTHROPIC_API_KEY").map(|k| !k.is_empty()).unwrap_or(false),
                        "pending": claude::login_pending()}),
                200,
            );
        }
        "/tutoriel" | "/tutoriel/" => return page(&["local", "tutorial.html"]), // montré une fois au premier lancement
        "/logs" | "/logs/" => return page(&["local", "logs.html"]),
        "/api/logs" => return send_json(&json!({"logs": jobs::log_list(), "job": job_status()}), 200),
        "/api/logs/rapport" => {
            return Reply {
                status: 200,
                headers: vec![
                    ("Content-Type".into(), "text/plain; charset=utf-8".into()),
                    (
                        "Content-Disposition".into(),
                        format!("attachment; filename=\"rapport-auto-montage-{}.txt\"", now_fmt("%Y-%m-%d_%Hh%M")),
                    ),
                ],
                body: Body::Bytes(jobs::report_text().into_bytes()),
            }
        }
        "/api/caption-style" => return send_json(&caption_style_info(), 200),
        "/api/style/analyse" => return send_json(&style_analysis(), 200),
        "/api/habillage" => return send_json(&Value::Object(habillage()), 200),
        "/api/son" => {
            let analyse = work().join("son_analyse.json");
            let a = if analyse.exists() { util::read_json(&analyse).unwrap_or(Value::Null) } else { Value::Null };
            // volume mesuré de la dernière vidéo entière (loudness.rs, work/volume.json)
            let volume = util::read_obj(work().join("volume.json")).get("montage_apercu.mp4").cloned().unwrap_or(Value::Null);
            return send_json(&json!({"settings": sound_settings(), "analyse": a, "volume": volume}), 200);
        }
        "/api/effects" => return send_json(&effects_info(), 200),
        "/api/status" => {
            return send_json(
                &json!({"version": *VERSION, "claude": claude::claude_connected(false),
                        "claudeInstalled": paths::claude().is_some(),
                        "styleAnalysis": read_text(work().join("analyse_style.md")),
                        "update": claude::update_summary(),
                        "tutorialSeen": tutorial_seen_path().exists(), "mode": app_mode(&work()),
                        "derushSubtitles": derush_subtitles(&work())}),
                200,
            )
        }
        "/api/projects" => return send_json(&projects_data(), 200),
        "/api/downloads" => return send_json(&downloads_data(), 200),
        "/api/place" => return send_json(&place::inventory(), 200),
        "/api/rushes" => return send_json(&rush_list(), 200),
        "/api/memoire" => {
            // mémoire nécessaire à la préparation d'un rush (avant de la lancer)
            let q = parse_qs(&req.query);
            let rushes_q: Vec<Option<String>> = q.iter().filter(|(k, _)| k == "rush").take(20).map(|(_, v)| safe_rush_name(v)).collect();
            if rushes_q.is_empty() || rushes_q.iter().any(|r| r.is_none()) || rushes_q.iter().any(|r| !rushes().join(r.as_ref().unwrap()).is_file()) {
                return err_json("Rush introuvable dans public/rushes.", 400);
            }
            let parts: Vec<String> = rushes_q.into_iter().map(Option::unwrap).collect();
            let instr: Vec<&String> = q.iter().filter(|(k, _)| k == "instructions").map(|(_, v)| v).collect();
            let with_claude = instr.len() == 1 && instr[0] == "1" && app_mode(&work()) != "derush";
            let steps = if with_claude { jobs::prepare_with_instructions() } else { jobs::prepare_steps() };
            let names: Vec<String> = steps.into_iter().map(|(n, _)| n).collect();
            let vpaths: Vec<PathBuf> = parts.iter().map(|n| rushes().join(n)).collect();
            let mut plan = memoire::summary(&names, &memoire::video_info(&vpaths, None));
            let kept = jobs::kept_parts(&parts);
            plan["kept"] = json!(jobs::abandon_parts_def().iter().filter(|(pid, _, _)| kept.iter().any(|k| k == pid)).map(|(_, l, _)| *l).collect::<Vec<_>>());
            return send_json(&plan, 200);
        }
        "/api/job/parts" => {
            // parties déjà faites d'une préparation en pause (« Abandonner »)
            let parts = if paused_task().is_some() { jobs::abandon_parts() } else { vec![] };
            return send_json(&json!({"parts": parts}), 200);
        }
        "/local/shim.js" | "/local/menu.js" => return send_file(req, Some(root().join("local").join(basename(path)))),
        "/api/regen" | "/api/job" => return send_json(&job_status(), 200),
        _ => {}
    }
    if let Some(rest) = path.strip_prefix("/tutoriel/") {
        // captures et petits rendus du tutoriel
        return send_file(req, inside(&root().join("local").join("tutoriel"), rest));
    }
    static LOG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/logs/([\w.-]+\.log)$").unwrap());
    if let Some(c) = LOG.captures(path) {
        let Some(target) = inside(&logs(), &c[1]).filter(|t| t.is_file()) else { return error(404, None) };
        return Reply {
            status: 200,
            headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into()), ("Cache-Control".into(), "no-store".into())],
            body: Body::Bytes(fs::read(target).unwrap_or_default()),
        };
    }
    static FONT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/api/fonts/([0-9a-f]{16})$").unwrap());
    if let Some(c) = FONT.captures(path) {
        let (_, files) = font_files();
        return match files.into_iter().find(|(id, _)| *id == c[1]) {
            Some((_, p)) => send_file(req, Some(p)),
            None => error(404, None),
        };
    }
    if let Some(rest) = path.strip_prefix("/style-analyse/") {
        return send_file(req, inside(&style_analyse_dir(), rest)); // planches de l'analyse
    }
    if path.starts_with("/sfx-defaut/") {
        // sons fournis (écoute sur la page « Rushes et montages »)
        return send_file(req, inside(&paths::public().join("sfx"), basename(path)));
    }
    if let Some(rest) = path.strip_prefix("/sfx/perso/") {
        return send_file(req, inside(&sounds_dir(), rest)); // sons personnels
    }
    if let Some(rest) = path.strip_prefix("/style/") {
        return send_file(req, inside(&style_videos_dir(), rest)); // planches des vidéos d'exemple
    }
    static THUMB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/api/projects/([^/]+)/thumb$").unwrap());
    if let Some(c) = THUMB.captures(path).filter(|c| safe_id(&c[1])) {
        let rel = Path::new(&c[1]).join("work").join("moments").join("thumbs");
        let folder = inside(&work().join("projets"), &rel.to_string_lossy());
        let names: Vec<String> = match &folder {
            Some(f) if f.is_dir() => util::list_dir(f).into_iter().filter(|n| n.starts_with("m_")).collect(),
            _ => vec![],
        };
        let file = folder.filter(|_| !names.is_empty()).map(|f| f.join(&names[names.len() / 3]));
        return send_file(req, file);
    }
    if let Some(rest) = path.strip_prefix("/out/") {
        return send_file(req, inside(&out(), rest));
    }
    static DB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/api/db/([^/]+)$").unwrap());
    if let Some(c) = DB.captures(path) {
        let coll = &c[1];
        if !safe_id(coll) {
            return err_json("collection invalide", 400);
        }
        let folder = db().join(coll);
        let mut docs = vec![];
        for name in util::list_dir(&folder) {
            if let Some(id) = name.strip_suffix(".json") {
                if let Some(data) = util::read_json(folder.join(&name)) {
                    docs.push(json!({"id": id, "data": data}));
                }
            }
        }
        return send_json(&json!({"docs": docs}), 200);
    }
    // Le reste : fichiers de la page des moments (extraits, vignettes, sons, police, montage).
    send_file(req, inside(&moments(), path.trim_start_matches('/')))
}

// --- Corps des requêtes -----------------------------------------------------------------
fn content_length(req: &Req) -> Result<u64, ()> {
    req.header("Content-Length").ok_or(())?.trim().parse::<u64>().map_err(|_| ())
}

/// Corps JSON de la requête (5 Mo au plus) ; Err s'il est invalide ou trop gros.
fn read_json(req: &Req, body: &mut dyn Read) -> Result<Value, ()> {
    let size = match req.header("Content-Length").map(str::trim).filter(|s| !s.is_empty()) {
        None => 0,
        Some(s) => s.parse::<i64>().map_err(|_| ())?,
    };
    if !(0..=JSON_MAX as i64).contains(&size) {
        return Err(());
    }
    let mut raw = vec![];
    body.take(size as u64).read_to_end(&mut raw).map_err(|_| ())?;
    if raw.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&raw).map_err(|_| ())
}

fn read_obj(req: &Req, body: &mut dyn Read) -> Map<String, Value> {
    match read_json(req, body) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Écrit `size` octets du corps dans `tmp` ; renvoie ce qui manque (0 : complet).
fn stream_to(body: &mut dyn Read, tmp: &Path, size: u64) -> u64 {
    let mut left = size;
    let res = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(tmp)?;
        let mut buf = vec![0u8; 4 << 20];
        while left > 0 {
            let want = left.min(buf.len() as u64) as usize;
            let n = body.read(&mut buf[..want])?;
            if n == 0 {
                break;
            }
            f.write_all(&buf[..n])?;
            left -= n as u64;
        }
        Ok(())
    })();
    if res.is_err() && left == 0 {
        left = 1; // page fermée pendant l'envoi, disque plein : pas de reste caché de plusieurs Go
    } else if res.is_err() {
        left = left.max(1);
    }
    left
}

/// Envoi d'un rush depuis la page (sélecteur de fichiers du navigateur), par morceaux.
fn upload_rush(req: &Req, body: &mut dyn Read, raw_name: &str) -> Reply {
    let Some(name) = safe_rush_name(raw_name) else {
        return err_json("Nom de fichier refusé : choisissez une vidéo (.mov, .mp4…).", 400);
    };
    let Ok(size) = content_length(req) else { return err_json("Taille du fichier inconnue.", 411) };
    util::mkdirs(rushes());
    if disk_free(&rushes()) < size + (2 << 30) {
        return err_json(&no_space(size + (2 << 30), &rushes()), 507);
    }
    let tmp = rushes().join(format!(".envoi-{name}"));
    if stream_to(body, &tmp, size) > 0 {
        let _ = fs::remove_file(&tmp);
        return err_json("Envoi interrompu.", 400);
    }
    if let Err(e) = fs::rename(&tmp, rushes().join(&name)) {
        return err_json(&e.to_string(), 500);
    }
    send_json(&json!({"ok": true, "name": name}), 200)
}

/// Corps de la requête écrit dans dest (par morceaux, via un fichier provisoire).
fn receive(req: &Req, body: &mut dyn Read, dest: &Path, max_size: Option<u64>) -> Option<String> {
    let Ok(size) = content_length(req) else { return Some("Taille du fichier inconnue.".into()) };
    if let Some(max) = max_size.filter(|m| size > *m) {
        return Some(format!("Fichier trop lourd (au plus {} Mo).", max >> 20));
    }
    let dir = dest.parent().unwrap_or(Path::new("."));
    util::mkdirs(dir);
    if disk_free(dir) < size + (1 << 30) {
        return Some(no_space(size + (1 << 30), dir));
    }
    let tmp = dir.join(format!(".envoi-{}", dest.file_name().unwrap_or_default().to_string_lossy()));
    if stream_to(body, &tmp, size) > 0 {
        let _ = fs::remove_file(&tmp);
        return Some("Envoi interrompu.".into());
    }
    if let Err(e) = fs::rename(&tmp, dest) {
        return Some(e.to_string());
    }
    None
}

static STYLE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w .()+,'&-]{1,200}$").unwrap());

fn ends_with_any(name: &str, exts: &[&str]) -> bool {
    let l = name.to_lowercase();
    exts.iter().any(|e| l.ends_with(e))
}

fn upload_style(req: &Req, body: &mut dyn Read, kind: &str, raw_name: &str) -> Reply {
    let name = basename(raw_name).trim().to_string();
    if name.is_empty() || name.starts_with('.') || !STYLE_NAME.is_match(&name) {
        return err_json("Nom de fichier refusé.", 400);
    }
    if kind == "sons" {
        if !ends_with_any(&name, &AUDIO_EXT) {
            return err_json("Choisissez un fichier son (.wav, .mp3, .m4a…).", 400);
        }
        let tmp = sounds_dir().join(format!(".source-{name}"));
        if let Some(e) = receive(req, body, &tmp, Some(100 << 20)) {
            return err_json(&e, 400);
        }
        let sid = sound_id(&name);
        let wav = sounds_dir().join(format!("{sid}.wav"));
        let (code, _, _) = procs::run(
            paths::ffmpeg(),
            [
                "-v".as_ref(),
                "error".as_ref(),
                "-y".as_ref(),
                "-i".as_ref(),
                tmp.as_os_str(),
                "-vn".as_ref(),
                "-t".as_ref(),
                "30".as_ref(),
                "-ac".as_ref(),
                "2".as_ref(),
                "-ar".as_ref(),
                "48000".as_ref(),
                wav.as_os_str(),
            ] as [&std::ffi::OsStr; 13],
        );
        let _ = fs::remove_file(&tmp);
        if code != 0 {
            return err_json("Ce fichier n'a pas pu être lu comme un son.", 400);
        }
        if moments().is_dir() {
            // écoute sur la page des moments déjà ouverte
            let mp3 = moments().join("sfx").join(format!("perso-{sid}.mp3"));
            util::mkdirs(moments().join("sfx"));
            procs::call(
                paths::ffmpeg(),
                [
                    "-v".as_ref(),
                    "error".as_ref(),
                    "-y".as_ref(),
                    "-i".as_ref(),
                    wav.as_os_str(),
                    "-c:a".as_ref(),
                    "libmp3lame".as_ref(),
                    "-b:a".as_ref(),
                    "96k".as_ref(),
                    mp3.as_os_str(),
                ] as [&std::ffi::OsStr; 10],
            );
        }
        return send_json(
            &json!({"ok": true, "id": format!("perso-{sid}"), "label": capitalize(&sid.replace('-', " ")), "style": style_info()}),
            200,
        );
    }
    if kind == "polices" {
        // police personnelle pour les sous-titres
        if !ends_with_any(&name, &fonts_lib::FONT_EXT) {
            return err_json("Choisissez un fichier de police (.ttf, .otf, .woff, .woff2).", 400);
        }
        util::mkdirs(fonts_lib::perso_dir());
        if let Some(e) = receive(req, body, &fonts_lib::perso_dir().join(&name), Some(30 << 20)) {
            return err_json(&e, 400);
        }
        return send_json(&caption_style_info(), 200);
    }
    if !is_video(&name) {
        return err_json("Choisissez une vidéo (.mp4, .mov…).", 400);
    }
    let dest = style_videos_dir().join(&name);
    if let Some(e) = receive(req, body, &dest, Some(4 << 30)) {
        return err_json(&e, 400);
    }
    std::thread::spawn(move || make_contact_sheet(&dest));
    send_json(&json!({"ok": true, "style": style_info()}), 200)
}

/// Consignes de montage (texte ou Markdown, 1 Mo au plus) -> work/instructions.md.
fn upload_instructions(req: &Req, body: &mut dyn Read) -> Reply {
    let header = unquote(req.header("X-File-Name").unwrap_or(""));
    let name = basename(if header.is_empty() { "instructions.md" } else { &header }).to_string();
    let size = req.header("Content-Length").and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
    if !ends_with_any(&name, &[".md", ".txt", ".markdown"]) || !(1..=(1 << 20)).contains(&size) {
        return err_json("Choisissez un fichier texte (.md ou .txt) de moins de 1 Mo.", 400);
    }
    let mut raw = vec![];
    let _ = body.take(size).read_to_end(&mut raw);
    let text = match String::from_utf8(raw) {
        Ok(t) => t,
        Err(e) => e.into_bytes().iter().map(|b| *b as char).collect(), // latin-1
    };
    util::mkdirs(work());
    let _ = fs::write(instructions_path(), text);
    let _ = fs::write(instructions_name_path(), &name);
    send_json(&json!({"ok": true, "instructions": instructions_info()}), 200)
}

static STYLE_ROUTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/api/style/(sons|videos|polices)/(.+)$").unwrap());

// --- DELETE -----------------------------------------------------------------------------
fn do_delete(req: &Req) -> Reply {
    if let Some(c) = STYLE_ROUTE.captures(&req.path) {
        let kind = &c[1];
        let name = basename(&c[2]).to_string();
        let paths_: Vec<Option<PathBuf>> = match kind {
            "sons" => vec![inside(&sounds_dir(), &format!("{}.wav", name.strip_prefix("perso-").unwrap_or(&name)))],
            "polices" => vec![inside(&fonts_lib::perso_dir(), &name)],
            _ => vec![inside(&style_videos_dir(), &name), inside(&style_videos_dir(), &format!("{}.planche.jpg", splitext(&name).0))],
        };
        for p in paths_.into_iter().flatten() {
            if p.is_file() {
                let _ = fs::remove_file(p);
            }
        }
        if kind == "polices" {
            return send_json(&caption_style_info(), 200);
        }
        return send_json(&json!({"ok": true, "style": style_info()}), 200);
    }
    if req.raw_path != "/api/instructions" {
        return error(404, None);
    }
    for p in [instructions_path(), instructions_name_path()] {
        if p.exists() {
            let _ = fs::remove_file(p);
        }
    }
    ok_json(200)
}

// --- PUT --------------------------------------------------------------------------------
fn do_put(req: &Req, body: &mut dyn Read) -> Reply {
    let route = req.raw_path.as_str();
    if route == "/api/mode" {
        // bandeau : Dérushage ou Auto-montage
        let b = read_obj(req, body);
        let mode = b.get("mode").and_then(Value::as_str).filter(|m| MODES.iter().any(|(c, _)| c == m));
        let Some(mode) = mode else { return err_json("Mode inconnu.", 400) };
        util::mkdirs(work());
        let _ = fs::write(work().join("mode.txt"), format!("{mode}\n"));
        return send_json(&json!({"mode": mode}), 200);
    }
    if route == "/api/derush" {
        // dérushage : sous-titres oui / non
        let b = read_obj(req, body);
        let Some(Value::Bool(subs)) = b.get("subtitles") else { return err_json("Réglage invalide", 400) };
        util::mkdirs(work());
        let _ = write_json(work().join("derush.json"), &json!({"subtitles": subs}), None, true);
        return send_json(&json!({"subtitles": subs}), 200);
    }
    if route == "/api/instructions" {
        return upload_instructions(req, body);
    }
    if ["/api/caption-style", "/api/effects", "/api/son", "/api/habillage"].contains(&route) {
        let Ok(Value::Object(b)) = read_json(req, body) else { return err_json("JSON invalide", 400) };
        util::mkdirs(work());
        return match route {
            "/api/habillage" => match save_habillage(&b) {
                Some(h) => send_json(&Value::Object(h), 200),
                None => err_json("Réglage invalide", 400),
            },
            "/api/son" => match save_sound(&b) {
                Some(s) => send_json(&json!({"settings": s}), 200),
                None => err_json("Réglage invalide", 400),
            },
            "/api/effects" => send_json(&save_effects(&b), 200),
            _ => {
                fonts_lib::save_style(&b);
                send_json(&caption_style_info(), 200)
            }
        };
    }
    if let Some(c) = STYLE_ROUTE.captures(&req.path) {
        return upload_style(req, body, &c[1], &c[2]);
    }
    static UP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/api/rushes/(.+)$").unwrap());
    if let Some(c) = UP.captures(&req.path) {
        return upload_rush(req, body, &c[1]);
    }
    static DOC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/api/db/([^/]+)/([^/]+)$").unwrap());
    let Some(c) = DOC.captures(&req.path).filter(|c| safe_id(&c[1]) && safe_id(&c[2])) else {
        return err_json("document invalide", 400);
    };
    let Ok(data) = read_json(req, body) else { return err_json("JSON invalide", 400) };
    let folder = db().join(&c[1]);
    util::mkdirs(&folder);
    let tmp = folder.join(format!(".{}.tmp", &c[2]));
    if write_json(&tmp, &data, Some(1), false).and_then(|_| fs::rename(&tmp, folder.join(format!("{}.json", &c[2])))).is_err() {
        return error(500, None);
    }
    ok_json(200)
}

// --- POST -------------------------------------------------------------------------------
const POST_ROUTES: &[&str] = &[
    "/api/regen", "/api/prepare", "/api/final", "/api/chat", "/api/chat/stop", "/api/chat/new",
    "/api/projects/open", "/api/projects/delete", "/api/projects/save", "/api/job/cancel", "/api/job/pause",
    "/api/job/resume", "/api/job/abandon", "/api/place/delete",
    "/api/claude/login", "/api/claude/code", "/api/claude/logout",
    "/api/update/check", "/api/update/apply", "/api/update/reset", "/api/restart", "/api/quit", "/api/style/analyse", "/api/son/analyse",
    "/api/tutorial/seen",
    "/api/moments/merge", "/api/moments/split", "/api/moments/unmerge", "/api/moments/unsplit",
];

fn do_post(req: &Req, body_reader: &mut dyn Read) -> Reply {
    let route = req.raw_path.as_str();
    if !POST_ROUTES.contains(&route) {
        return error(404, None);
    }
    let body = read_obj(req, body_reader);
    match route {
        "/api/update/check" => {
            let mut r = claude::update_check();
            if let Value::Object(m) = &mut r {
                m.insert("version".into(), json!(*VERSION));
                m.insert("onDisk".into(), json!(disk_version()));
            }
            return send_json(&r, 200);
        }
        "/api/restart" => {
            // fichiers changés sur le disque : relance (recompilée si lancée depuis les sources)
            if jobs::is_running() {
                return err_json("Une tâche est en cours : attendez qu'elle finisse.", 409);
            }
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(1));
                crate::relance::relaunch();
            });
            return ok_json(202);
        }
        "/api/quit" => {
            // « auto-montage stop » (menu « Arrêter Auto-montage ») : une préparation en cours est
            // mise en pause (reprise au prochain lancement), une autre tâche arrêtée.
            if jobs::is_running() && !cancel_job(true) {
                cancel_job(false);
            }
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(2));
                std::process::exit(0);
            });
            return ok_json(202);
        }
        "/api/update/apply" => {
            let r = claude::update_check();
            let err = r.get("error").filter(|e| util::truthy(Some(e)));
            if err.is_some() || !util::truthy(r.get("newer")) {
                if err.is_none() && disk_version() != *VERSION {
                    return send_json(
                        &json!({"error": format!("Les fichiers d'Auto-montage sont déjà en version {}, mais il tourne encore en {} : redémarrez-le.", disk_version(), *VERSION), "restart": true}),
                        400,
                    );
                }
                let msg = err.map(|e| py_str(Some(e))).unwrap_or_else(|| "Vous avez déjà la dernière version.".into());
                return err_json(&msg, 400);
            }
            let tag = py_str(r.get("tag"));
            let steps: Vec<Step> = vec![("mise à jour".into(), vec!["update".into(), "apply".into(), tag.clone()])];
            if !start_job("update", steps, &tag, None, None, false) {
                return err_json(&busy_error(), 409);
            }
            return send_json(&json!({"ok": true, "tag": tag}), 202);
        }
        "/api/update/reset" => {
            if body.get("confirm") != Some(&json!(true)) {
                // la page demande toujours une confirmation avant
                return err_json("Réinitialisation non confirmée.", 400);
            }
            let tag = format!("v{}", *VERSION); // version qui tourne (VERSION sur le disque a pu être modifié)
            let steps: Vec<Step> = vec![("réinitialisation".into(), vec!["update".into(), "reset".into(), tag.clone()])];
            if !start_job("reset", steps, &tag, None, None, false) {
                return err_json(&busy_error(), 409);
            }
            return send_json(&json!({"ok": true, "tag": tag}), 202);
        }
        "/api/claude/login" => {
            if paths::claude().is_none() {
                return err_json("Claude Code n'est pas installé : relancez Auto-montage pour l'installer.", 400);
            }
            let out = claude::login_start(util::truthy(body.get("console")));
            let status = if out.get("error").is_some() { 400 } else { 200 };
            return send_json(&out, status);
        }
        "/api/claude/code" => {
            let code = body_str(&body, "code").trim().to_string();
            if code.is_empty() || code.chars().count() > 500 {
                return err_json("Collez le code affiché après l'autorisation.", 400);
            }
            let out = claude::login_code(&code);
            let status = if out.get("error").is_some() { 400 } else { 200 };
            return send_json(&out, status);
        }
        "/api/claude/logout" => {
            claude::logout();
            return ok_json(200);
        }
        "/api/job/cancel" => {
            if !cancel_job(false) {
                return err_json("Aucune tâche en cours.", 409);
            }
            return ok_json(200);
        }
        "/api/job/pause" => {
            if !cancel_job(true) {
                return err_json("Aucune préparation en cours.", 409);
            }
            return ok_json(200);
        }
        "/api/job/resume" => {
            return match resume_job() {
                Some(e) => err_json(&e, 409),
                None => ok_json(202),
            };
        }
        "/api/job/abandon" => {
            // parties à effacer (toutes sans liste)
            let delete = match body.get("delete") {
                None | Some(Value::Null) => None,
                Some(Value::Array(a)) if a.iter().all(Value::is_string) => {
                    Some(a.iter().map(|s| s.as_str().unwrap().to_string()).collect::<Vec<_>>())
                }
                _ => return err_json("Choix invalide.", 400),
            };
            let _g = JOB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            if jobs::is_running() || paused_task().is_none() {
                return err_json("Aucune préparation en pause.", 409);
            }
            let kept = jobs::abandon_task(delete);
            return send_json(&json!({"ok": true, "kept": kept}), 200);
        }
        _ => {}
    }
    if let Some(action) = route.strip_prefix("/api/projects/") {
        let pid = body_str(&body, "id");
        if action != "save" && !safe_id(&pid) {
            return err_json("montage invalide", 400);
        }
        // pas pendant une préparation, une génération ou une création 4K
        let _g = JOB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if busy() {
            return err_json(&busy_error(), 409);
        }
        let args: Vec<&str> = if action != "save" { vec![action, &pid] } else { vec![action] };
        let (ok, msg) = run_project(&args);
        return if ok { send_json(&json!({"ok": true, "message": msg}), 200) } else { err_json(&msg, 400) };
    }
    if route == "/api/place/delete" {
        let ids: Vec<String> = match body.get("ids") {
            Some(Value::Array(a)) if !a.is_empty() && a.iter().all(Value::is_string) => a.iter().map(|s| s.as_str().unwrap().to_string()).collect(),
            _ => return err_json("Rien de choisi.", 400),
        };
        // une tâche en cours écrit dans ces dossiers (et ses fichiers .part.*)
        let _g = JOB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if jobs::is_running() {
            return err_json("Une tâche est en cours : attendez qu'elle finisse.", 409);
        }
        if let Some(rec) = paused_task() {
            // vidéos d'une préparation en pause : gardées pour la reprendre
            let args: Vec<String> = util::arr(&Value::Object(rec.clone()), "args").iter().filter_map(|s| s.as_str().map(String::from)).collect();
            if ids.iter().any(|i| args.iter().any(|a| *i == format!("rush:{a}"))) {
                return err_json(
                    &format!(
                        "La préparation de {} est en pause : ses vidéos sont gardées tant qu'elle n'est pas reprise ou abandonnée.",
                        py_str(rec.get("arg"))
                    ),
                    409,
                );
            }
        }
        let before = disk_free(root());
        let done = place::delete(&ids);
        let freed = disk_free(root()).saturating_sub(before);
        return send_json(&json!({"ok": true, "deleted": done, "freed": freed}), 200);
    }
    if route == "/api/final" {
        if !util::truthy(final_info().get("available")) {
            return err_json("Pas encore de vidéo entière générée : générez-la d'abord.", 400);
        }
        if !start_job("final", jobs::final_steps(), "4k", None, None, false) {
            return err_json(&busy_error(), 409);
        }
        return ok_json(202);
    }
    if route == "/api/chat" || route == "/api/style/analyse" {
        if let Some(msg) = derush_refused() {
            return err_json(msg, 409);
        }
    }
    match route {
        "/api/chat" => {
            let message = body_str(&body, "message").trim().to_string();
            if message.is_empty() {
                return err_json("Message vide.", 400);
            }
            if paths::claude().is_none() {
                return err_json("Claude Code n'est pas installé : relancez Auto-montage pour l'installer.", 400);
            }
            if !claude::claude_connected(false) {
                return err_json("Claude Code n'est pas connecté : connectez-le sur la page « Connexion à Claude ».", 400);
            }
            return match claude::chat_send(message.chars().take(20000).collect()) {
                Some(e) => err_json(&e, 409),
                None => ok_json(202),
            };
        }
        "/api/chat/stop" => {
            claude::chat_stop();
            return ok_json(200);
        }
        "/api/chat/new" => {
            if !claude::chat_reset() {
                return err_json("Arrêtez d'abord la réponse en cours.", 409);
            }
            return ok_json(200);
        }
        "/api/tutorial/seen" => {
            util::mkdirs(work());
            let _ = fs::write(tutorial_seen_path(), format!("{}\n", now_iso_secs()));
            return ok_json(200);
        }
        "/api/son/analyse" => {
            // quelques secondes : faite tout de suite
            if !edit_json().exists() || read_text(work().join("rush.txt")).map(|r| r.is_empty()).unwrap_or(true) {
                return err_json("Pas encore de vidéo entière générée.", 400);
            }
            let mut c = std::process::Command::new(paths::self_exe());
            c.arg("analyze_sound").current_dir(root());
            return match claude::run_timeout(c, std::time::Duration::from_secs(600)) {
                Ok((0, _, _)) => send_json(
                    &json!({"settings": sound_settings(), "analyse": util::read_json(work().join("son_analyse.json")).unwrap_or(Value::Null)}),
                    200,
                ),
                Ok((_, _, err)) => err_json(err.trim().lines().last().unwrap_or("échec"), 500),
                Err(e) => err_json(&e, 500),
            };
        }
        "/api/style/analyse" => {
            let videos = style_videos();
            if videos.is_empty() {
                return err_json("Ajoutez d'abord des vidéos d'exemple.", 400);
            }
            if !claude::claude_connected(false) {
                return err_json("Claude Code n'est pas connecté : connectez-le sur la page « Connexion à Claude ».", 400);
            }
            if !start_job("style", jobs::style_steps(), &format!("{} vidéo(s) d'exemple", videos.len()), None, None, false) {
                return err_json(&busy_error(), 409);
            }
            return ok_json(202);
        }
        "/api/prepare" => return post_prepare(&body),
        _ => {}
    }
    if let Some(action) = route.strip_prefix("/api/moments/") {
        return post_moments(action, &body);
    }
    // /api/regen
    let variant = {
        let v = body_str(&body, "variant");
        if v.is_empty() {
            "travail".to_string()
        } else {
            v
        }
    };
    if !safe_id(&variant) {
        return err_json("variante invalide", 400);
    }
    let doc = db().join("variantes").join(format!("{variant}.json"));
    if !doc.exists() {
        // Variante jamais modifiée sur la page (rush tout juste préparé) : sa sélection
        // actuelle (les suggestions) est envoyée avec la demande.
        let Some(keep) = body.get("keep").filter(|k| k.is_array()) else {
            return err_json(&format!("variante « {variant} » pas encore enregistrée"), 400);
        };
        util::mkdirs(doc.parent().unwrap());
        let name = {
            let n = body_str(&body, "name");
            if n.is_empty() {
                variant.clone()
            } else {
                n
            }
        };
        let rush = basename(&read_text(work().join("rush.txt")).unwrap_or_default()).to_string();
        let _ = write_json(&doc, &json!({"name": name, "keep": keep, "rush": rush, "updatedAt": now_iso()}), Some(1), false);
    }
    if !start_job("regen", jobs::regen_steps(), &variant, None, None, false) {
        return err_json(&busy_error(), 409);
    }
    ok_json(202)
}

/// Scinder ou fusionner des moments (decoupage), puis refaire la page des moments (seuls les
/// extraits touchés sont recalculés).
fn post_moments(action: &str, body: &Map<String, Value>) -> Reply {
    if busy() {
        return err_json(&busy_error(), 409);
    }
    // Fusion de plusieurs moments sélectionnés sur la frise : « count » fusions de suite
    // (le moment fusionné garde l'id du premier).
    let parsed = (|| {
        let mid = py_int(body.get("id")?)?;
        let t = if action == "split" { py_float(body.get("t").unwrap_or(&json!(0)))? } else { 0.0 };
        let count = if action == "merge" { py_int(body.get("count").unwrap_or(&json!(1)))?.clamp(1, 100) } else { 1 };
        Some((mid, t, count))
    })();
    let Some((mid, t, count)) = parsed else { return err_json("Moment invalide.", 400) };
    let mut done = 0;
    let mut message = String::new();
    for _ in 0..count {
        let r = match action {
            "merge" => decoupage::merge(mid),
            "unmerge" => decoupage::unmerge(mid),
            "split" => decoupage::split(mid, t),
            _ => decoupage::unsplit(mid),
        };
        match r {
            Ok(m) => {
                message = m;
                done += 1;
            }
            Err(e) => {
                if done == 0 {
                    return err_json(&e.to_string(), 400);
                }
                message = format!("{} moments fusionnés en un seul (pas plus : {e})", done + 1);
                break;
            }
        }
    }
    if count > 1 && done == count {
        message = format!("{} moments fusionnés en un seul.", count + 1);
    }
    let steps: Vec<Step> = vec![("page des moments".into(), vec!["make_moments".into()])];
    if !start_job("decoupage", steps, &message, None, None, false) {
        return err_json(&busy_error(), 409);
    }
    send_json(&json!({"ok": true, "message": message}), 202)
}

fn post_prepare(body: &Map<String, Value>) -> Reply {
    // Une vidéo (« rush ») ou plusieurs (« rushes », dans l'ordre) : assemblées en un rush.
    let raw: Vec<Value> = match body.get("rushes").filter(|v| util::truthy(Some(v))) {
        Some(Value::Array(a)) => a.clone(),
        Some(_) => vec![],
        None => vec![body.get("rush").cloned().unwrap_or(Value::Null)],
    };
    let parts: Vec<Option<String>> = if (1..=20).contains(&raw.len()) {
        raw.iter().map(|r| safe_rush_name(r.as_str().unwrap_or(""))).collect()
    } else {
        vec![None]
    };
    if parts.iter().any(|p| p.as_ref().map(|n| !rushes().join(n).is_file()).unwrap_or(true)) {
        return err_json("Rush introuvable dans public/rushes.", 400);
    }
    let parts: Vec<String> = parts.into_iter().map(Option::unwrap).collect();
    let name = if parts.len() > 1 { crate::join_rushes::joined_name(&parts) } else { parts[0].clone() };
    if parts.len() > 1 {
        let need: u64 = parts.iter().map(|n| fs::metadata(rushes().join(n)).map(|m| m.len()).unwrap_or(0)).sum::<u64>() + (2 << 30);
        if disk_free(&rushes()) < need {
            return err_json(&no_space(need, &rushes()), 507);
        }
    }
    let langue = {
        let l = body_str(body, "langue");
        if l.is_empty() {
            "fr".to_string()
        } else {
            l
        }
    };
    if language_label(&langue).is_none() {
        return err_json("Langue inconnue.", 400);
    }
    let mut steps = jobs::prepare_steps();
    if util::truthy(body.get("instructions")) && app_mode(&work()) != "derush" {
        // dérushage : sans Claude
        if instructions_info().is_null() && !jobs::style_has_content() {
            return err_json("Aucune consigne ni aucun élément de style choisi.", 400);
        }
        if !claude::claude_connected(false) {
            return err_json("Claude Code n'est pas connecté : connectez-le sur la page « Connexion à Claude ».", 400);
        }
        steps = jobs::prepare_with_instructions();
    }
    let abandon = util::truthy(body.get("abandon"));
    let rec = if jobs::is_running() { None } else { paused_task() };
    if let Some(rec) = &rec {
        if rec.get("args") == Some(&json!(parts)) && !abandon {
            // Même rush que la préparation en pause : elle reprend là où elle en était.
            return match resume_job() {
                Some(e) => err_json(&e, 409),
                None => send_json(&json!({"ok": true, "resumed": true}), 202),
            };
        }
        if !abandon {
            // la page demande d'abord confirmation
            return send_json(&json!({"error": busy_error(), "paused": true}), 409);
        }
        jobs::abandon_task(None);
    }
    if !jobs::is_running() {
        // langue de la transcription, lue par prepare
        util::mkdirs(work());
        let _ = fs::write(work().join("langue.txt"), format!("{langue}\n"));
    }
    let reuse = !jobs::kept_parts(&parts).is_empty();
    if !start_job("prepare", steps, &name, Some(parts), None, reuse) {
        return err_json(&busy_error(), 409);
    }
    ok_json(202)
}

// --- Requête : contrôles, routes, réponse -----------------------------------------------
/// Refuse ce qui ne vient pas de l'interface elle-même :
/// - un autre nom que localhost (DNS rebinding : site piégé dont le nom pointe vers 127.0.0.1,
///   qui pourrait sinon lire et piloter l'interface) ;
/// - une écriture (POST, PUT, DELETE) envoyée par la page d'un autre site (CSRF).
fn check_request(req: &Req) -> Option<Reply> {
    let host = req.header("Host").unwrap_or("").trim().to_lowercase();
    let name = if host.ends_with(']') { host.clone() } else { host.rsplit_once(':').map(|(a, _)| a.to_string()).unwrap_or(host.clone()) };
    if !ALLOWED_HOSTS.contains(&name) {
        return Some(error(403, Some("Adresse non autorisée : ouvrez http://localhost")));
    }
    if req.method != "GET" && req.method != "HEAD" {
        let origin = req.header("Origin");
        let fetch_site = req.header("Sec-Fetch-Site");
        let other_origin = origin.map(|o| netloc(o).to_lowercase() != host).unwrap_or(false);
        if other_origin || matches!(fetch_site, Some("cross-site") | Some("same-site")) {
            return Some(error(403, Some("Requête d'un autre site refusée")));
        }
    }
    None
}

/// `urlparse(url).netloc`
fn netloc(url: &str) -> &str {
    let is_scheme = |s: &str| {
        s.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
            && s.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    };
    let rest = if let Some(r) = url.strip_prefix("//") {
        r
    } else {
        match url.split_once("://") {
            Some((scheme, r)) if is_scheme(scheme) => r,
            _ => return "",
        }
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    &rest[..end]
}

pub fn handle(mut request: tiny_http::Request) {
    let method = request.method().as_str().to_uppercase();
    let url = request.url().to_string();
    let (raw_path, query) = {
        let no_frag = url.split('#').next().unwrap_or("");
        match no_frag.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (no_frag.to_string(), String::new()),
        }
    };
    let headers: Vec<(String, String)> =
        request.headers().iter().map(|h| (h.field.as_str().as_str().to_lowercase(), h.value.as_str().to_string())).collect();
    let req = Req { method: method.clone(), path: unquote(&raw_path), raw_path, query, headers };
    let reply = match check_request(&req) {
        Some(r) => r,
        None => {
            let body = request.as_reader();
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match method.as_str() {
                "GET" => do_get(&req),
                "PUT" => do_put(&req, body),
                "POST" => do_post(&req, body),
                "DELETE" => do_delete(&req),
                other => error(501, Some(&format!("Unsupported method ({})", pyjson::dumps(&json!(other)).replace('"', "'")))),
            }));
            run.unwrap_or_else(|_| error(500, None))
        }
    };
    if reply.status >= 400 {
        // journal court : seulement les erreurs
        let addr = request.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
        eprintln!("{addr} - - [{}] \"{method} {url} HTTP/{}\" {} -", now_fmt("%d/%b/%Y %H:%M:%S"), request.http_version(), reply.status);
    }
    send(request, reply);
}

fn send(request: tiny_http::Request, reply: Reply) {
    use tiny_http::{Header, Response, StatusCode};
    let mut headers: Vec<Header> = vec![];
    for (k, v) in reply.headers.iter().chain(
        [
            // Pas d'affichage dans le cadre d'un autre site (clic piégé) ; types de fichiers tels quels.
            ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
            ("X-Frame-Options".to_string(), "DENY".to_string()),
            ("Content-Security-Policy".to_string(), "frame-ancestors 'none'".to_string()),
        ]
        .iter(),
    ) {
        if k.eq_ignore_ascii_case("Content-Length") {
            continue;
        }
        if let Ok(h) = Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            headers.push(h);
        }
    }
    let status = StatusCode(reply.status);
    let _ = match reply.body {
        Body::Bytes(b) => {
            let n = b.len();
            request.respond(Response::new(status, headers, std::io::Cursor::new(b), Some(n), None).with_chunked_threshold(usize::MAX))
        }
        Body::File(f, n) => request.respond(Response::new(status, headers, f.take(n), Some(n as usize), None).with_chunked_threshold(usize::MAX)),
        Body::Empty => request.respond(Response::new(status, headers, std::io::empty(), Some(0), None).with_chunked_threshold(usize::MAX)),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, host: &str, origin: Option<&str>) -> Req {
        let mut headers = vec![("host".to_string(), host.to_string())];
        if let Some(o) = origin {
            headers.push(("origin".into(), o.into()));
        }
        Req { method: method.into(), raw_path: "/".into(), path: "/".into(), query: String::new(), headers }
    }

    #[test]
    fn hosts_and_origins() {
        assert!(check_request(&req("GET", "localhost:8080", None)).is_none());
        assert!(check_request(&req("GET", "[::1]:8080", None)).is_none());
        assert_eq!(check_request(&req("GET", "evil.com", None)).map(|r| r.status), Some(403));
        assert!(check_request(&req("POST", "localhost:8080", Some("http://localhost:8080"))).is_none());
        assert_eq!(check_request(&req("POST", "localhost:8080", Some("http://evil.com"))).map(|r| r.status), Some(403));
        assert_eq!(check_request(&req("POST", "localhost:8080", Some("null"))).map(|r| r.status), Some(403));
        assert_eq!(netloc("http://Localhost:8080/x"), "Localhost:8080");
        assert_eq!(netloc("null"), "");
        assert_eq!(netloc("//h:1"), "h:1");
    }
}
