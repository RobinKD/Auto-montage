//! Montages enregistrés : met de côté le montage du rush en cours et le rouvre plus tard.
//!
//! Un montage, c'est tout ce qui a été calculé ou choisi pour un rush : versions de travail
//! (1080p, source 4K), transcription, page des moments, choix de la page (variantes, corrections,
//! effets, découpes), choix de montage, rendus et discussion avec Claude. Le mettre de côté le
//! range dans work/projets/<rush>/ ; le rouvrir le remet en place sans rien recalculer.
//!
//! Usage : auto-montage project save            met de côté le montage en cours
//!         auto-montage project open <dossier>   rouvre un montage (l'actuel est mis de côté)
//!         auto-montage project delete <dossier> supprime un montage enregistré
//!         auto-montage project list             montages enregistrés (JSON)
//! prepare appelle « save » quand le rush change ; la page « Nouveau rush » (interface locale)
//! appelle les autres.
//!
//! Fonctions utilisables par les autres modules : save, open_project, delete, listing, summary.
//! Elles renvoient le message à afficher (ou l'erreur) au lieu de l'écrire.

use crate::moments_lib::copy2;
use crate::paths;
use crate::pyjson;
use crate::util::{self, read_json, round_to};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const USAGE: &str = "Usage : auto-montage project save            met de côté le montage en cours
        auto-montage project open <dossier>   rouvre un montage (l'actuel est mis de côté)
        auto-montage project delete <dossier> supprime un montage enregistré
        auto-montage project list             montages enregistrés (JSON)";

/// Fichiers de work/ communs à tous les rushes : jamais déplacés.
/// Les consignes (work/instructions.*) et la langue (langue.txt) sont choisies avant de préparer le
/// rush suivant : elles restent (langue_rush.txt, la langue du rush, est rangée avec lui).
/// tache.json et reprise : préparation en cours ou en pause (local_server) : jamais rangée.
/// derush_suite : ce que la préparation du rush passe à celle de la vidéo du dérush (derush_video).
/// Le mode (mode.txt : dérushage ou auto-montage) est propre à chaque montage : rangé avec lui.
pub const SHARED: [&str; 21] = [
    "projets", "local_db", "timings.json", "models", "regen_trigger.txt", "instructions.md", "instructions.name", "langue.txt",
    "derush.json", "style", "logs", "sauvegardes", "caption_style_default.json", "effets_page.json", "tutoriel_vu", "rendu_hyperframes",
    "rendu_parties", "memoire_mesures.json", "tache.json", "reprise", "derush_suite",
];
/// Hors de work/ : déplacés (lourds) ou copiés (suivis par git, restent en place).
pub const MOVED: [&str; 8] = [
    "public/rushes/rush_1080.mp4",
    "public/rushes/rush_1080.webm",
    "public/rushes/rush_2160.mp4",
    "public/rushes/rush_2160.webm", // source 4K des versions précédentes (VP9)
    "public/audio",
    "out/montage_apercu.mp4",
    "out/montage_4k.mp4",
    "out/export", // copies pour les plateformes (export.rs)
];
pub const COPIED: [&str; 2] = ["src/data/edit.json", "src/data/face.json"];

fn store() -> PathBuf {
    paths::work().join("projets")
}

/// Chemin relatif écrit avec « / » (MOVED, COPIED) sous `base`, avec les séparateurs du système.
fn rel(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, part| p.join(part))
}

fn read(path: &Path) -> Option<String> {
    util::read_text(path)
}

/// `os.path.lexists`
fn lexists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn size_of(path: &Path) -> u64 {
    if path.is_file() {
        return fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.file_type().is_dir())
        .map(|e| fs::metadata(e.path()).map(|m| m.len()).unwrap_or(0))
        .sum()
}

/// Copie récursive qui garde les liens symboliques (repli de `shutil.move` entre deux disques).
fn copy_any(src: &Path, dst: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        {
            return std::os::unix::fs::symlink(fs::read_link(src)?, dst);
        }
        #[cfg(not(unix))]
        {
            if src.is_dir() {
                return util::copy_tree(src, dst);
            }
            return fs::copy(src, dst).map(|_| ());
        }
    }
    if meta.is_dir() {
        fs::create_dir_all(dst)?;
        for e in fs::read_dir(src)? {
            let e = e?;
            copy_any(&e.path(), &dst.join(e.file_name()))?;
        }
        Ok(())
    } else {
        copy2(src, dst)
    }
}

/// `shutil.move` : renommage, sinon copie puis suppression (autre disque).
fn shutil_move(src: &Path, dst: &Path) -> io::Result<()> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_any(src, dst)?;
    util::remove_all(src);
    Ok(())
}

/// Déplace src vers dst (remplacé s'il existe), si src existe.
fn move_(src: &Path, dst: &Path) -> io::Result<()> {
    if lexists(src) {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        let meta = fs::symlink_metadata(dst);
        match meta {
            Ok(m) if m.is_dir() => fs::remove_dir_all(dst)?,
            Ok(_) => fs::remove_file(dst)?,
            Err(_) => {}
        }
        shutil_move(src, dst)?;
    }
    Ok(())
}

/// Choix de la page des moments (work/local_db) qui visent ce rush : (collection, fichier).
fn page_docs(rush: &str) -> Vec<(String, String)> {
    let db = paths::work().join("local_db");
    let mut out = vec![];
    for coll in util::list_dir(&db) {
        let folder = db.join(&coll);
        if !folder.is_dir() {
            continue;
        }
        for name in util::list_dir(&folder) {
            if !name.ends_with(".json") {
                continue;
            }
            if let Some(Value::Object(doc)) = read_json(folder.join(&name)) {
                if doc.get("rush").and_then(Value::as_str) == Some(rush) {
                    out.push((coll.clone(), name));
                }
            }
        }
    }
    out
}

/// Description du montage en cours (pour la liste des montages enregistrés).
pub fn summary() -> Map<String, Value> {
    let work = paths::work();
    let rush = read(&work.join("rush.txt")).unwrap_or_default();
    let mut info = Map::new();
    info.insert("rush".into(), json!(Path::new(&rush).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    info.insert("key".into(), read(&work.join("rush_key.txt")).map(Value::String).unwrap_or(Value::Null));
    info.insert("savedAt".into(), json!(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S+00:00").to_string()));
    let duration = read_json(paths::data().join("edit.json")).and_then(|e| {
        let frames = e.get("durationInFrames")?.as_f64()?;
        let fps = e.get("fps")?.as_f64()?;
        if fps == 0.0 {
            return None;
        }
        Some(util::num(round_to(frames / fps, 1)))
    });
    info.insert("duration".into(), duration.unwrap_or(Value::Null));
    info.insert("has4k".into(), json!(paths::out().join("montage_4k.mp4").exists()));
    info.insert("hasPage".into(), json!(work.join("moments").join("index.html").exists()));
    info
}

static UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_.-]+").unwrap());

fn folder_name(rush: &str) -> String {
    let stem = Path::new(rush).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = UNSAFE.replace_all(&stem, "_").into_owned();
    if stem.is_empty() {
        "rush".into()
    } else {
        stem
    }
}

fn err(e: io::Error) -> String {
    e.to_string()
}

/// Met de côté le montage en cours ; ne fait rien s'il n'y en a pas (None).
/// Renvoie (nom du dossier, message).
pub fn save() -> Result<Option<(String, String)>, String> {
    let root = paths::root();
    let work = paths::work();
    if read(&work.join("rush.txt")).map(|r| r.is_empty()).unwrap_or(true) {
        return Ok(None);
    }
    let mut info = summary();
    let rush = info["rush"].as_str().unwrap_or("").to_string();
    let name = folder_name(&rush);
    let dest = store().join(&name);
    if dest.is_dir() {
        let old_key = read(&dest.join("projet.json"))
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("key").and_then(Value::as_str).map(String::from));
        let key = info["key"].as_str().map(String::from);
        match (&old_key, &key) {
            (Some(o), Some(k)) if !o.is_empty() && !k.is_empty() && o != k => {
                // Autre vidéo sous le même nom : l'ancien montage garde sa place, sous un autre nom.
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                fs::rename(&dest, store().join(format!("{name}-{stamp}"))).map_err(err)?;
            }
            _ => fs::remove_dir_all(&dest).map_err(err)?, // même rush, enregistré plus tôt : remplacé par l'état actuel
        }
    }
    fs::create_dir_all(&dest).map_err(err)?;
    for entry in util::list_dir(&work) {
        if !SHARED.contains(&entry.as_str()) {
            move_(&work.join(&entry), &dest.join("work").join(&entry)).map_err(err)?;
        }
    }
    for r in MOVED {
        move_(&rel(root, r), &rel(&dest, r)).map_err(err)?;
    }
    // Vidéos du dernier rendu (liens ou copies de rush_1080/rush_2160) : elles garderaient sur le
    // disque les vidéos de ce montage, même supprimé ; refaites au prochain rendu.
    let assets = work.join("rendu_hyperframes").join("assets");
    for entry in util::list_dir(&assets) {
        let low = entry.to_lowercase();
        if [".mp4", ".webm", ".mov"].iter().any(|e| low.ends_with(e)) {
            fs::remove_file(assets.join(&entry)).map_err(err)?;
        }
    }
    for r in COPIED {
        let src = rel(root, r);
        if src.exists() {
            let dst = rel(&dest, r);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(err)?;
            }
            copy2(&src, &dst).map_err(err)?;
        }
    }
    for (coll, fname) in page_docs(&rush) {
        move_(&work.join("local_db").join(&coll).join(&fname), &dest.join("local_db").join(&coll).join(&fname)).map_err(err)?;
    }
    info.insert("size".into(), json!(size_of(&dest)));
    let text = pyjson::dumps_with(&Value::Object(info), pyjson::Opts { indent: Some(1), ensure_ascii: false, sort_keys: false });
    fs::write(dest.join("projet.json"), text).map_err(err)?;
    Ok(Some((name.clone(), format!("Montage de {rush} mis de côté dans work/projets/{name}"))))
}

/// Nom de dossier acceptable (pas de chemin, pas de dossier caché) avec un projet.json.
fn valid(name: &str) -> bool {
    !(name.is_empty() || name.contains('/') || name.contains('\\') || name.starts_with('.') || !store().join(name).join("projet.json").is_file())
}

/// Rouvre un montage enregistré (l'actuel est mis de côté). Renvoie les messages à afficher.
pub fn open_project(name: &str) -> Result<String, String> {
    let root = paths::root();
    let work = paths::work();
    if !valid(name) {
        return Err(format!("Montage enregistré introuvable : {name}"));
    }
    let mut src = store().join(name);
    let info = read_json(src.join("projet.json")).ok_or_else(|| format!("Montage enregistré illisible : {name}"))?;
    let info_rush = info.get("rush").and_then(Value::as_str).unwrap_or("").to_string();
    let mut said = vec![];
    if let Some(current) = read(&work.join("rush.txt")).filter(|r| !r.is_empty()) {
        let base = Path::new(&current).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let key = read(&work.join("rush_key.txt"));
        let info_key = info.get("key").and_then(Value::as_str).map(String::from);
        if base == info_rush && key == info_key {
            // Même rush préparé à nouveau entre-temps : l'état actuel remplace l'ancien.
            fs::remove_dir_all(&src).map_err(err)?;
            return Ok(format!("{info_rush} est déjà le rush en cours"));
        }
        // Mis à l'écart le temps d'enregistrer le montage en cours (qui peut prendre ce nom).
        let aside = store().join(format!(".ouverture-{name}"));
        fs::rename(&src, &aside).map_err(err)?;
        src = aside;
        if let Some((_, msg)) = save()? {
            said.push(msg);
        }
    }
    let src_work = src.join("work");
    for entry in util::list_dir(&src_work) {
        move_(&src_work.join(&entry), &work.join(&entry)).map_err(err)?;
    }
    for r in MOVED {
        let target = rel(root, r);
        let from = rel(&src, r);
        if lexists(&from) {
            move_(&from, &target).map_err(err)?;
        } else if target.is_dir() {
            fs::remove_dir_all(&target).map_err(err)?;
        } else if lexists(&target) {
            fs::remove_file(&target).map_err(err)?;
        }
    }
    for r in COPIED {
        let from = rel(&src, r);
        if from.exists() {
            copy2(&from, &rel(root, r)).map_err(err)?;
        }
    }
    let db = src.join("local_db");
    for coll in util::list_dir(&db) {
        for fname in util::list_dir(db.join(&coll)) {
            move_(&db.join(&coll).join(&fname), &work.join("local_db").join(&coll).join(&fname)).map_err(err)?;
        }
    }
    fs::remove_dir_all(&src).map_err(err)?;
    let rush = read(&work.join("rush.txt")).filter(|r| !r.is_empty()).unwrap_or_else(|| "-".into());
    let missing = !root.join(&rush).exists();
    said.push(format!(
        "Montage de {info_rush} rouvert{}",
        if missing { " (vidéo d'origine absente de public/rushes : la 4K ne pourra pas être refaite)" } else { "" }
    ));
    Ok(said.join("\n"))
}

/// Supprime un montage enregistré.
pub fn delete(name: &str) -> Result<String, String> {
    if !valid(name) {
        return Err(format!("Montage enregistré introuvable : {name}"));
    }
    fs::remove_dir_all(store().join(name)).map_err(err)?;
    Ok(format!("Montage {name} supprimé"))
}

/// Montages enregistrés, du plus récent au plus ancien (projet.json + id, rushPresent, thumb).
pub fn listing() -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    for name in util::list_dir(store()) {
        let Some(Value::Object(mut info)) = read_json(store().join(&name).join("projet.json")) else { continue };
        let rush_file = paths::rushes().join(info.get("rush").and_then(Value::as_str).unwrap_or(""));
        let thumbs = store().join(&name).join("work").join("moments").join("thumbs");
        let has_thumb = util::list_dir(&thumbs).iter().any(|n| n.starts_with("m_"));
        info.insert("id".into(), json!(name));
        info.insert("rushPresent".into(), json!(rush_file.is_file()));
        info.insert("thumb".into(), if has_thumb { json!(format!("/api/projects/{name}/thumb")) } else { Value::Null });
        out.push(Value::Object(info));
    }
    let saved = |v: &Value| v.get("savedAt").and_then(Value::as_str).unwrap_or("").to_string();
    out.sort_by_key(|v| std::cmp::Reverse(saved(v))); // stable, comme sorted(reverse=True)
    out
}

pub fn main(args: &[String]) -> i32 {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let result = match (cmd, args.len()) {
        ("save", _) => save().map(|r| r.map(|(_, m)| m).unwrap_or_default()),
        ("open", 2) => open_project(&args[1]),
        ("delete", 2) => delete(&args[1]),
        ("list", _) => Ok(pyjson::dumps_with(&Value::Array(listing()), pyjson::Opts { indent: Some(1), ensure_ascii: false, sort_keys: false })),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(msg) => {
            if !msg.is_empty() {
                println!("{msg}");
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names() {
        assert_eq!(folder_name("essai.mov"), "essai");
        assert_eq!(folder_name("mon rush (2).mov"), "mon_rush_2_");
        assert_eq!(folder_name("été.mp4"), "_t_");
    }
}
