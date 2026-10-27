//! Mises à jour d'Auto-montage depuis les versions publiées sur GitHub (page « Mises à jour »).
//!
//! Usage : auto-montage update check          dernière version publiée (JSON)
//!         auto-montage update apply <vX.Y>   télécharge cette version et remplace les
//!                                            fichiers du programme
//!         auto-montage update reset [vX.Y]   réinitialisation : retélécharge la version
//!                                            installée (par défaut) et remet les fichiers
//!                                            du programme modifiés ou effacés comme publiés
//! Le code de la version (archive du tag) remplace celui du dossier d'installation ; ce qui
//! appartient à l'utilisateur reste : montage/work (montages, réglages), montage/out (rendus),
//! rushes, sons personnels, police Oliver, paquets Node. Les fichiers retirés d'une version à
//! l'autre sont effacés (liste .auto-montage-files). La réinitialisation garde une copie des
//! fichiers qu'elle remplace dans montage/work/sauvegardes/reinitialisation-<date>/.
//!
//! Le programme lui-même (`montage/bin/auto-montage`) est remplacé par celui compilé pour ce
//! système et joint à la version (relance.rs) ; sans lui, recompilé depuis les sources si Rust est
//! installé. Il est téléchargé et vérifié avant que le moindre fichier soit remplacé. Le serveur
//! local relance ensuite la nouvelle version (`relance::relaunch`).
//!
//! Essais : `AM_UPDATE_API` remplace https://api.github.com (faux GitHub local).

use crate::net;
use crate::paths;
use crate::relance;
use crate::progress::{report, say};
use crate::pyjson;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

const USAGE: &str = "Mises à jour d'Auto-montage depuis les versions publiées sur GitHub (page « Mises à jour »).

Usage : auto-montage update check          dernière version publiée (JSON)
        auto-montage update apply <vX.Y>   télécharge cette version et remplace les
                                           fichiers du programme
        auto-montage update reset [vX.Y]   réinitialisation : retélécharge la version
                                           installée (par défaut) et remet les fichiers
                                           du programme modifiés ou effacés comme publiés";

fn project() -> PathBuf {
    // dossier d'installation (celui qui contient montage/)
    let root = paths::root();
    let abs = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    abs.parent().map(Path::to_path_buf).unwrap_or(abs)
}
fn api_base() -> String {
    std::env::var("AM_UPDATE_API").map(|s| s.trim_end_matches('/').to_string()).unwrap_or_else(|_| "https://api.github.com".into())
}
fn repo() -> String {
    std::env::var("AM_UPDATE_REPO").unwrap_or_else(|_| "RobinKD/Auto-montage".into())
}
fn backups() -> PathBuf {
    paths::work().join("sauvegardes")
}
fn manifest() -> PathBuf {
    project().join(".auto-montage-files")
}
/// Jamais remplacés ni effacés : les données de l'utilisateur et ce qui est installé à part.
const KEEP: [&str; 11] = [
    "montage/work/",
    "montage/out/",
    "montage/public/rushes/",
    "montage/public/sfx/perso/",
    "montage/node_modules/",
    "montage/whisper.cpp/",
    "montage/public/audio/",
    // Programme compilé et ffmpeg livré (pas dans l'archive).
    "montage/bin/",
    "montage/rust/target/",
    // Montage en cours (réécrits par build_edit) : pas remplacés par ceux du dépôt.
    "montage/src/data/edit.json",
    "montage/src/data/face.json",
];
const SKIP: [&str; 2] = ["packaging/", ".github/"]; // pas dans les installations

static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^v\d+(\.\d+)*$").unwrap());
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());

fn version_tuple(v: &str) -> Vec<u128> {
    let v = if v.is_empty() { "0" } else { v };
    DIGITS.find_iter(v).map(|m| m.as_str().parse().unwrap_or(u128::MAX)).collect()
}

pub fn current_version() -> String {
    fs::read_to_string(project().join("VERSION")).map(|s| s.trim().to_string()).unwrap_or_else(|_| "0".into())
}

const API_HEADERS: [(&str, &str); 3] =
    [("Accept", "application/vnd.github+json"), ("User-Agent", "auto-montage-updater"), ("X-GitHub-Api-Version", "2022-11-28")];

/// Erreur d'une requête : code HTTP (urllib.error.HTTPError) ou réseau.
enum ApiError {
    Http(u16),
    Net(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ApiError::Http(c) => write!(f, "HTTP Error {c}"),
            ApiError::Net(e) => write!(f, "{e}"),
        }
    }
}

fn api(url: &str) -> Result<ureq::Response, ApiError> {
    let mut req = net::agent(Duration::from_secs(60)).get(url);
    for (k, v) in API_HEADERS {
        req = req.set(k, v);
    }
    match req.call() {
        Ok(r) => Ok(r),
        Err(ureq::Error::Status(code, _)) => Err(ApiError::Http(code)),
        Err(e) => Err(ApiError::Net(e.to_string())),
    }
}

/// {"current", "latest", "tag", "newer", "notes", "error"}.
fn check() -> Value {
    let mut out = Map::new();
    out.insert("current".into(), json!(current_version()));
    for (k, v) in [("latest", Value::Null), ("tag", Value::Null), ("newer", json!(false)), ("notes", json!("")), ("error", Value::Null)] {
        out.insert(k.into(), v);
    }
    let releases = api(&format!("{}/repos/{}/releases?per_page=20", api_base(), repo())).and_then(|r| {
        let mut body = String::new();
        r.into_reader().read_to_string(&mut body).map_err(|e| ApiError::Net(e.to_string()))?;
        serde_json::from_str::<Value>(&body).map_err(|e| ApiError::Net(e.to_string()))
    });
    let releases = match releases {
        Ok(r) => r,
        Err(ApiError::Http(code)) => {
            out.insert(
                "error".into(),
                json!(if code == 403 || code == 429 {
                    "GitHub limite les vérifications depuis cette connexion : réessayez dans une heure.".to_string()
                } else {
                    format!("GitHub ne répond pas correctement (erreur {code}).")
                }),
            );
            return Value::Object(out);
        }
        Err(ApiError::Net(e)) => {
            out.insert("error".into(), json!(format!("Pas de connexion à GitHub ({e}).")));
            return Value::Object(out);
        }
    };
    let tag_of = |r: &Value| r.get("tag_name").and_then(Value::as_str).unwrap_or("").to_string();
    let tags: Vec<&Value> = releases
        .as_array()
        .map(|a| a.iter().filter(|r| TAG.is_match(&tag_of(r)) && !crate::util::truthy(r.get("draft"))).collect())
        .unwrap_or_default();
    if tags.is_empty() {
        out.insert("error".into(), json!("Aucune version publiée trouvée."));
        return Value::Object(out);
    }
    // max() de Python : le premier des plus grands.
    let mut best = tags[0];
    for r in &tags[1..] {
        if version_tuple(&tag_of(r)) > version_tuple(&tag_of(best)) {
            best = r;
        }
    }
    let current = version_tuple(out["current"].as_str().unwrap_or("0"));
    // Nouveautés de toutes les versions plus récentes que celle installée (la plus récente en
    // premier), pour qui saute plusieurs versions.
    let mut newer: Vec<&Value> = tags.iter().copied().filter(|r| version_tuple(&tag_of(r)) > current).collect();
    newer.sort_by(|a, b| version_tuple(&tag_of(b)).cmp(&version_tuple(&tag_of(a))));
    if newer.is_empty() {
        newer = vec![best];
    }
    let notes: Vec<String> = newer
        .iter()
        .take(15)
        .map(|r| {
            let tag = tag_of(r);
            let body = r.get("body").and_then(Value::as_str).unwrap_or("").replace("\r\n", "\n");
            let body = body.split("\n## Télécharger").next().unwrap_or("").replace("## Nouveautés", "");
            format!("## Version {}\n{}", &tag[1..], body.trim())
        })
        .collect();
    let notes: String = notes.join("\n\n").chars().take(20000).collect();
    let best_tag = tag_of(best);
    out.insert("latest".into(), json!(best_tag[1..]));
    out.insert("tag".into(), json!(best_tag));
    out.insert("notes".into(), json!(notes));
    out.insert("newer".into(), json!(version_tuple(&best_tag) > current));
    Value::Object(out)
}

fn protected(rel: &str) -> bool {
    KEEP.iter().any(|k| rel.starts_with(k)) || rel.starts_with("montage/public/fonts/Oliver")
}

fn skipped(rel: &str) -> bool {
    SKIP.iter().any(|k| rel.starts_with(k))
}

/// Archive .tar.gz de la version, lue en entier (barre « Téléchargement »).
fn download(tag: &str) -> Result<Vec<u8>, ApiError> {
    say(&format!("Téléchargement d'Auto-montage {}…", &tag[1..]));
    // essais : archive .tar.gz locale au lieu de GitHub
    let (mut reader, total): (Box<dyn Read>, u64) = match std::env::var("AM_UPDATE_ARCHIVE") {
        Ok(local) if !local.is_empty() => {
            let f = fs::File::open(&local).map_err(|e| ApiError::Net(format!("{local} : {e}")))?;
            let size = f.metadata().map(|m| m.len()).unwrap_or(0);
            (Box::new(f), size)
        }
        _ => {
            let r = api(&format!("{}/repos/{}/tarball/{tag}", api_base(), repo()))?;
            let size = r.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(0);
            (Box::new(r.into_reader()), size)
        }
    };
    let mut buf = vec![];
    let mut chunk = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut chunk).map_err(|e| ApiError::Net(e.to_string()))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        let got = buf.len() as u64;
        report("Téléchargement", got as f64, total.max(got).max(1) as f64);
    }
    say(&format!("Archive reçue ({} ko)…", buf.len() >> 10));
    Ok(buf)
}

/// Dossier temporaire effacé à la fin (tempfile.TemporaryDirectory).
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<TempDir> {
        let base = std::env::temp_dir();
        for i in 0..1000u32 {
            let p = base.join(format!("auto-montage-maj-{}-{}-{i}", std::process::id(), crate::util::now_secs() as u64));
            if fs::create_dir(&p).is_ok() {
                return Ok(TempDir(p));
            }
        }
        fs::create_dir(base.join("auto-montage-maj")).map(|_| TempDir(base.join("auto-montage-maj")))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Extrait l'archive dans tmp : (dossier source, fichiers du programme à installer).
fn unpack(buf: &[u8], tmp: &Path) -> Result<(PathBuf, Vec<String>), String> {
    let open = || tar::Archive::new(flate2::read::GzDecoder::new(buf));
    // Vérification de tous les noms avant d'extraire quoi que ce soit.
    let mut archive = open();
    let entries = archive.entries().map_err(|e| format!("Archive illisible : {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Archive illisible : {e}"))?;
        let t = entry.header().entry_type();
        if !(t.is_file() || t.is_dir() || t == tar::EntryType::Continuous) {
            continue;
        }
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        // archive GitHub : un dossier racine « owner-repo-sha/ »
        if name.starts_with('/') || name.split('/').any(|p| p == "..") {
            return Err(format!("Archive suspecte : {name}"));
        }
    }
    let mut archive = open();
    let entries = archive.entries().map_err(|e| format!("Archive illisible : {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("Archive illisible : {e}"))?;
        let t = entry.header().entry_type();
        if !(t.is_file() || t.is_dir() || t == tar::EntryType::Continuous) {
            continue;
        }
        entry.unpack_in(tmp).map_err(|e| format!("Archive illisible : {e}"))?;
    }
    let roots = crate::util::list_dir(tmp);
    if roots.len() != 1 {
        return Err("Archive inattendue.".into());
    }
    let src = tmp.join(&roots[0]);
    let mut files = vec![];
    for e in walkdir::WalkDir::new(&src).into_iter().flatten() {
        if e.file_type().is_dir() {
            continue;
        }
        let Ok(rel) = e.path().strip_prefix(&src) else { continue };
        let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let rel = rel.join("/");
        if skipped(&rel) || protected(&rel) {
            continue;
        }
        files.push(rel);
    }
    if !files.iter().any(|f| f == "VERSION") || !src.join("montage").join("rust").join("Cargo.toml").exists() {
        return Err("L'archive ne ressemble pas à Auto-montage : rien n'est remplacé.".into());
    }
    files.sort();
    Ok((src, files))
}

fn on_disk(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, x| p.join(x))
}

fn install(src: &Path, rel: &str) -> Result<(), String> {
    let dest = on_disk(&project(), rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{} : {e}", parent.display()))?;
    }
    let tmp_dest = PathBuf::from(format!("{}.maj", dest.display()));
    // contenu seul, pas les droits de l'archive
    copy_content(&on_disk(src, rel), &tmp_dest).map_err(|e| format!("{rel} : {e}"))?;
    fs::rename(&tmp_dest, &dest).map_err(|e| format!("{rel} : {e}"))?;
    #[cfg(unix)]
    if rel.ends_with(".sh") {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// shutil.copyfile : le contenu seul (fichier créé avec les droits par défaut).
fn copy_content(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut r = fs::File::open(from)?;
    let mut w = fs::File::create(to)?;
    std::io::copy(&mut r, &mut w)?;
    Ok(())
}

// --- Programme compilé de la version ----------------------------------------------------

/// Programme de la nouvelle version, préparé avant de remplacer les fichiers.
enum Program {
    /// téléchargé et vérifié, à mettre en place (relance::commit)
    Ready(PathBuf),
    /// pas publié pour ce système : à recompiler une fois les sources installées
    Build,
    /// rien à faire ici (essai avec AM_UPDATE_ARCHIVE)
    Skip,
}

/// Programme publié pour ce système avec la version `tag`, téléchargé (barre « Téléchargement
/// du programme ») et vérifié ; None s'il n'est pas publié.
/// Adresse de téléchargement du fichier `name` joint à la version `tag` sur GitHub ; None si
/// la version ou le fichier n'existe pas.
pub fn release_asset(tag: &str, name: &str) -> Result<Option<String>, String> {
    Ok(release_asset_info(tag, name)?.map(|(url, _)| url))
}

/// Comme release_asset, avec l'empreinte SHA-256 que GitHub donne pour ce fichier (« digest »
/// des versions récentes ; None pour les plus anciennes).
pub fn release_asset_info(tag: &str, name: &str) -> Result<Option<(String, Option<String>)>, String> {
    let release = match api(&format!("{}/repos/{}/releases/tags/{tag}", api_base(), repo())) {
        Ok(r) => {
            let mut body = String::new();
            r.into_reader().read_to_string(&mut body).map_err(|e| format!("GitHub : {e}"))?;
            serde_json::from_str::<Value>(&body).map_err(|e| format!("GitHub : {e}"))?
        }
        Err(ApiError::Http(404)) => return Ok(None),
        Err(e) => return Err(format!("Téléchargement impossible : {e}")),
    };
    let asset = release.get("assets").and_then(Value::as_array).and_then(|a| {
        a.iter().find(|x| x.get("name").and_then(Value::as_str) == Some(name))
    });
    let digest = asset
        .and_then(|a| a.get("digest"))
        .and_then(Value::as_str)
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(|d| d.to_lowercase());
    Ok(asset.and_then(|a| a.get("browser_download_url")).and_then(Value::as_str).map(|u| (u.to_string(), digest)))
}

fn fetch_program(tag: &str) -> Result<Option<PathBuf>, String> {
    let Some(name) = relance::asset_name() else { return Ok(None) };
    let Some((url, digest)) = release_asset_info(tag, &name)? else { return Ok(None) };
    let url = url.as_str();
    say(&format!("Téléchargement du programme ({name})…"));
    let resp = match net::agent(Duration::from_secs(600)).get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => return Err(format!("Téléchargement du programme impossible (erreur {code}).")),
        Err(e) => return Err(format!("Téléchargement du programme impossible : {e}")),
    };
    let total: u64 = resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(0);
    let dl = relance::installed().with_file_name(format!("{name}.telechargement"));
    if let Some(dir) = dl.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    }
    let result = (|| -> Result<(), String> {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        let mut file = fs::File::create(&dl).map_err(|e| format!("{} : {e}", dl.display()))?;
        let mut reader = resp.into_reader();
        let mut chunk = vec![0u8; 1 << 16];
        let mut got = 0u64;
        loop {
            let n = reader.read(&mut chunk).map_err(|e| format!("Téléchargement du programme coupé : {e}"))?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut file, &chunk[..n]).map_err(|e| format!("{} : {e}", dl.display()))?;
            hash.update(&chunk[..n]);
            got += n as u64;
            report("Téléchargement du programme", got as f64, total.max(got).max(1) as f64);
        }
        // empreinte publiée par GitHub pour ce fichier : programme reçu tel quel
        if let Some(expected) = &digest {
            let got = hex::encode(hash.finalize());
            if &got != expected {
                return Err(format!("Programme téléchargé abîmé ou modifié (empreinte {got} au lieu de {expected}) : rien n'est remplacé."));
            }
        }
        Ok(())
    })();
    let staged = result.and_then(|_| relance::stage(&dl, &tag[1..]));
    let _ = fs::remove_file(&dl);
    staged.map(Some)
}

/// Rien à faire pour le programme : essai avec une archive locale sans faux GitHub.
fn skip_program() -> bool {
    std::env::var_os("AM_UPDATE_ARCHIVE").is_some() && std::env::var_os("AM_UPDATE_API").is_none()
}

fn prepare_program(tag: &str) -> Result<Program, String> {
    if skip_program() {
        return Ok(Program::Skip);
    }
    match fetch_program(tag)? {
        Some(p) => Ok(Program::Ready(p)),
        None if relance::can_build() => Ok(Program::Build),
        None => Err(format!(
            "Le programme d'Auto-montage {} n'est pas publié pour ce système ({}) et Rust n'est pas installé pour le compiler : rien n'est remplacé.",
            &tag[1..],
            relance::asset_name().unwrap_or_else(|| format!("{} {}", std::env::consts::OS, std::env::consts::ARCH))
        )),
    }
}

/// Recompile le programme depuis les sources installées ; dans une installation (programme dans
/// bin/), le met à la place de l'ancien.
fn build_program(tag: &str) -> Result<(), String> {
    say("Compilation du programme (plusieurs minutes la première fois)…");
    let built = relance::build()?;
    if relance::installed().is_file() {
        let staged = relance::stage(&built, &tag[1..])?;
        relance::commit(&staged)?;
    }
    Ok(())
}

fn finish_program(program: Program, tag: &str) -> Result<(), String> {
    match program {
        Program::Ready(p) => {
            relance::commit(&p)?;
            say(&format!("Programme {} installé.", &tag[1..]));
            Ok(())
        }
        Program::Build => build_program(tag),
        Program::Skip => Ok(()),
    }
}

/// Réinitialisation : remet le programme publié avec cette version s'il a changé.
fn reset_program(tag: &str) {
    if skip_program() {
        return;
    }
    match fetch_program(tag) {
        Ok(Some(p)) => {
            if relance::same_as_installed(&p) {
                let _ = fs::remove_file(&p);
                say("Programme déjà identique à celui publié.");
            } else if let Err(e) = relance::commit(&p) {
                say(&format!("Programme non remplacé : {e}"));
            } else {
                say(&format!("Programme {} remis comme publié.", &tag[1..]));
            }
        }
        Ok(None) if relance::can_build() => {
            if let Err(e) = build_program(tag) {
                say(&format!("Programme non recompilé : {e}"));
            }
        }
        Ok(None) => say("Programme pas publié pour ce système : gardé tel quel."),
        Err(e) => say(&format!("Programme non vérifié : {e}")),
    }
}


fn fail(msg: impl std::fmt::Display) -> i32 {
    eprintln!("{msg}");
    1
}

fn write_manifest(files: &[String], tag: &str) -> Result<(), String> {
    fs::write(manifest(), files.join("\n")).map_err(|e| format!(".auto-montage-files : {e}"))?;
    fs::write(project().join(".auto-montage-version"), format!("{}\n", &tag[1..])).map_err(|e| format!(".auto-montage-version : {e}"))
}

fn apply(tag: &str) -> i32 {
    if !TAG.is_match(tag) {
        return fail(format!("Version invalide : {tag}"));
    }
    let buf = match download(tag) {
        Ok(b) => b,
        Err(e) => return fail(format!("Téléchargement impossible : {e}")),
    };
    // programme téléchargé et vérifié avant de toucher aux fichiers
    let program = match prepare_program(tag) {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    say("Installation…");
    let tmp = match TempDir::new() {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    let (src, files) = match unpack(&buf, &tmp.0) {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    let old: BTreeSet<String> = fs::read_to_string(manifest()).map(|s| s.split('\n').map(String::from).collect()).unwrap_or_default();
    for (i, rel) in files.iter().enumerate() {
        if let Err(e) = install(&src, rel) {
            return fail(e);
        }
        report("Installation", (i + 1) as f64, files.len() as f64);
    }
    let mut removed = 0;
    let new: BTreeSet<&String> = files.iter().collect();
    let base = paths::canon(&project()).unwrap_or_else(|_| project());
    for rel in old.iter().filter(|r| !new.contains(r)) {
        // retirés dans la nouvelle version
        let Ok(path) = paths::canon(&on_disk(&project(), rel)) else { continue };
        if !path.starts_with(&base) || path == base {
            continue; // liste modifiée : rien hors de l'installation
        }
        if !rel.is_empty() && !protected(rel) && !skipped(rel) && path.is_file() && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    if let Err(e) = write_manifest(&files, tag) {
        return fail(e);
    }
    drop(tmp);
    if let Err(e) = finish_program(program, tag) {
        return fail(e);
    }
    say(&format!("Auto-montage {} installé : {} fichiers remplacés, {removed} retirés.", &tag[1..], files.len()));
    0
}

fn same(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(ma), Ok(mb)) if ma.len() == mb.len() => fs::read(a).ok() == fs::read(b).ok(),
        _ => false,
    }
}

/// Remet les fichiers du programme comme dans la version publiée (celle installée par
/// défaut) : fichiers modifiés ou effacés depuis l'installation, par exemple par Claude Code
/// lancé dans le dossier. Les données de l'utilisateur (KEEP) ne sont ni lues ni touchées ;
/// chaque fichier remplacé est d'abord copié dans work/sauvegardes/.
fn reset(tag: Option<&str>) -> i32 {
    let mut tag = tag.map(String::from).unwrap_or_else(|| format!("v{}", current_version()));
    let mut buf = None;
    if TAG.is_match(&tag) {
        match download(&tag) {
            Ok(b) => buf = Some(b),
            Err(ApiError::Http(404)) => {}
            Err(e) => return fail(format!("Téléchargement impossible : {e}")),
        }
    }
    let buf = match buf {
        Some(b) => b,
        None => {
            // Version installée pas publiée (fichiers d'une branche de travail, VERSION abîmé) :
            // la dernière version publiée.
            let latest = check();
            let Some(t) = latest.get("tag").and_then(Value::as_str) else {
                return fail(latest.get("error").and_then(Value::as_str).unwrap_or("Aucune version publiée trouvée."));
            };
            say(&format!(
                "La version {} n'est pas publiée sur GitHub : réinitialisation avec la dernière version publiée, {}.",
                tag.get(1..).unwrap_or(""),
                latest["latest"].as_str().unwrap_or("")
            ));
            tag = t.to_string();
            match download(&tag) {
                Ok(b) => b,
                Err(e) => return fail(format!("Téléchargement impossible : {e}")),
            }
        }
    };
    say("Comparaison des fichiers…");
    let tmp = match TempDir::new() {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    let (src, files) = match unpack(&buf, &tmp.0) {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    let (mut changed, mut missing) = (vec![], vec![]);
    for (i, rel) in files.iter().enumerate() {
        let dest = on_disk(&project(), rel);
        if !dest.is_file() {
            missing.push(rel.clone());
        } else if !same(&dest, &on_disk(&src, rel)) {
            changed.push(rel.clone());
        }
        report("Comparaison", (i + 1) as f64, files.len() as f64);
    }
    let mut backup = None;
    if !changed.is_empty() {
        let b = backups().join(format!("reinitialisation-{}", chrono::Local::now().format("%Y-%m-%d_%H%M%S")));
        for rel in &changed {
            let to = on_disk(&b, rel);
            if let Some(parent) = to.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(e) = copy_content(&on_disk(&project(), rel), &to) {
                return fail(format!("{rel} : {e}"));
            }
        }
        backup = Some(b);
    }
    let todo: Vec<&String> = changed.iter().chain(missing.iter()).collect();
    for (i, rel) in todo.iter().enumerate() {
        if let Err(e) = install(&src, rel) {
            return fail(e);
        }
        report("Réinitialisation", (i + 1) as f64, todo.len() as f64);
    }
    if let Err(e) = write_manifest(&files, &tag) {
        return fail(e);
    }
    drop(tmp);
    for rel in &changed {
        say(&format!("  remis comme publié : {rel}"));
    }
    for rel in &missing {
        say(&format!("  recréé : {rel}"));
    }
    reset_program(&tag);
    if let Some(b) = backup {
        let abs = std::path::absolute(&b).unwrap_or(b);
        let rel = abs.strip_prefix(project()).map(Path::to_path_buf).unwrap_or(abs.clone());
        say(&format!("Copie des fichiers remplacés : {}", rel.display()));
    }
    let n = |k: usize, word: &str| format!("{k} {word}{}", if k > 1 { "s" } else { "" });
    say(&format!(
        "Auto-montage {} réinitialisé : {} remis comme publiés, {}, {}.",
        &tag[1..],
        n(changed.len(), "fichier"),
        n(missing.len(), "recréé"),
        n(files.len() - todo.len(), "déjà identique")
    ));
    0
}

pub fn main(args: &[String]) -> i32 {
    match (args.first().map(String::as_str), args.len()) {
        (Some("check"), _) => {
            say(&pyjson::dumps_utf8(&check()));
            0
        }
        (Some("apply"), 2) => apply(&args[1]),
        (Some("reset"), 1 | 2) => reset(args.get(1).map(String::as_str)),
        _ => fail(USAGE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(version_tuple("v0.10") > version_tuple("v0.9"));
        assert_eq!(version_tuple(""), vec![0]);
        assert!(TAG.is_match("v0.68") && !TAG.is_match("v0.68-rc") && !TAG.is_match("0.68"));
    }

    #[test]
    fn keep_list() {
        assert!(protected("montage/work/edit.json"));
        assert!(protected("montage/public/fonts/Oliver-Regular.ttf"));
        assert!(!protected("montage/selection/page.html"));
        assert!(skipped(".github/workflows/tests.yml"));
    }
}
