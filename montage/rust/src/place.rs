//! Place prise sur le disque par Auto-montage, et ce qu'on peut supprimer pour en libérer
//! (page « Rushes et montages », section « Libérer de la place »).
//!
//! Éléments proposés :
//!   projet:<dossier>     montage enregistré (work/projets/<dossier>)
//!   rush:<fichier>       vidéo envoyée (public/rushes), copie de l'original de l'ordinateur
//!   hd                   source 4K du montage en cours (public/rushes/rush_2160.webm), refaite à la
//!                        prochaine création 4K
//!   rendu                copies des vidéos laissées par le dernier rendu (work/rendu_hyperframes/assets),
//!                        refaites au prochain rendu
//!   parties              parties d'un long rendu interrompu (work/rendu_parties), refaites si effacées
//!   sauvegarde:<dossier> fichiers remplacés par une réinitialisation (work/sauvegardes)
//!   restes               envois coupés (.envoi-*), fichiers d'une tâche arrêtée en route (*.part.*) et
//!                        images extraites par HyperFrames restées dans le dossier temporaire
//!
//! Usage : auto-montage place                  liste (JSON)
//!         auto-montage place delete <id>…     supprime (seulement des éléments de la liste)
//! Le serveur ne l'appelle que quand aucune tâche ne tourne (« restes » serait sinon une tâche en cours).

use crate::paths;
use crate::pyjson;
use crate::util::{self, read_json, read_text};
use regex::Regex;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

fn store() -> PathBuf {
    paths::work().join("projets")
}
fn assets() -> PathBuf {
    paths::work().join("rendu_hyperframes").join("assets")
}
pub fn parts_dir() -> PathBuf {
    paths::work().join("rendu_parties") // parties d'un long rendu interrompu
}
fn backups() -> PathBuf {
    paths::work().join("sauvegardes")
}

pub const VIDEO_EXT: [&str; 7] = [".mov", ".mp4", ".m4v", ".mkv", ".webm", ".avi", ".mts"];
pub static DERIVED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^rush_(1080|2160)\.").unwrap()); // versions de travail

pub fn is_video(name: &str) -> bool {
    let l = name.to_lowercase();
    VIDEO_EXT.iter().any(|e| l.ends_with(e))
}

fn nlink(m: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.nlink()
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        1
    }
}

/// Place rendue par la suppression de path : les fichiers qui ont un autre nom ailleurs (lien
/// physique, comme les vidéos du rendu) ne libèrent rien tant que l'autre nom reste.
pub fn freed(path: &Path) -> u64 {
    let Ok(m) = fs::symlink_metadata(path) else { return 0 };
    if m.file_type().is_symlink() {
        return 0;
    }
    if m.is_file() {
        return if nlink(&m) == 1 { m.len() } else { 0 };
    }
    let mut total = 0;
    for e in walkdir::WalkDir::new(path).into_iter().flatten() {
        if let Ok(m) = e.metadata() {
            if m.is_file() && nlink(&m) == 1 {
                total += m.len();
            }
        }
    }
    total
}

/// Images extraites par HyperFrames gardées dans le dossier temporaire (rendus d'avant la
/// 0.59, ou lancés à la main) : jusqu'à plus de 10 Go.
pub fn extract_caches() -> Vec<PathBuf> {
    let tmp = std::env::temp_dir();
    util::list_dir(&tmp)
        .into_iter()
        .filter(|n| n.starts_with("hyperframes-extract-cache-"))
        .map(|n| tmp.join(n))
        .collect()
}

pub fn leftovers() -> Vec<PathBuf> {
    let mut out = vec![];
    for d in [paths::rushes(), paths::out()] {
        for n in util::list_dir(&d) {
            if n.starts_with(".envoi-") || n.contains(".part.") {
                out.push(d.join(n));
            }
        }
    }
    out.extend(extract_caches());
    out
}

pub fn render_copies() -> Vec<PathBuf> {
    util::list_dir(assets()).into_iter().filter(|n| is_video(n)).map(|n| assets().join(n)).collect()
}

pub fn render_parts() -> Vec<PathBuf> {
    util::list_dir(parts_dir()).into_iter().map(|n| parts_dir().join(n)).collect()
}

fn day(ts: f64) -> String {
    chrono::DateTime::from_timestamp(ts as i64, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y").to_string())
        .unwrap_or_default()
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn inventory() -> Value {
    let current = read_text(paths::work().join("rush.txt")).map(|r| name_of(Path::new(&r))).unwrap_or_default();
    let mut projects: Vec<(String, Value)> = vec![];
    for name in util::list_dir(store()) {
        if let Some(info) = read_json(store().join(&name).join("projet.json")) {
            projects.push((name, info));
        }
    }
    let mut items: Vec<Value> = vec![];
    for (name, info) in &projects {
        let rush = util::s(info, "rush").filter(|s| !s.is_empty()).unwrap_or(name);
        let saved = util::s(info, "savedAt")
            .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok().or_else(|| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()))
            .map(|d| format!("mis de côté le {}", d.format("%d/%m/%Y")))
            .unwrap_or_default();
        items.push(json!({"id": format!("projet:{name}"), "group": "montages", "label": rush, "size": freed(&store().join(name)),
                          "detail": saved,
                          "note": "Choix, versions de travail et rendus effacés ; la vidéo envoyée reste (ci-dessous)."}));
    }
    let mut names = util::list_dir(paths::rushes());
    names.sort_by_key(|n| n.to_lowercase());
    for name in names {
        let path = paths::rushes().join(&name);
        if !path.is_file() || !is_video(&name) || DERIVED.is_match(&name) || name.starts_with('.') || name.contains(".part.") {
            continue;
        }
        let used = projects.iter().any(|(_, info)| util::s(info, "rush") == Some(name.as_str()));
        let note = if name == current {
            format!(
                "Vidéo du montage en cours : sans elle, sa 4K ne pourra plus être créée{} Le montage lui-même reste.",
                if paths::rushes().join("rush_2160.webm").exists() {
                    " (la source 4K est déjà prête, elle suffit tant qu'elle reste)."
                } else {
                    "."
                }
            )
        } else if used {
            "Utilisée par un montage enregistré : il se rouvrira, mais sa 4K ne pourra plus être refaite.".into()
        } else {
            "Aucun montage ne l'utilise.".into()
        };
        items.push(json!({"id": format!("rush:{name}"), "group": "videos", "label": name, "size": freed(&path),
                          "detail": format!("fichier du {}", day(util::mtime(&path).unwrap_or(0.0))), "note": note,
                          "keep": name == current || used}));
    }
    let hd = paths::rushes().join("rush_2160.webm");
    if hd.is_file() {
        items.push(json!({"id": "hd", "group": "videos", "label": "Source 4K du montage en cours", "size": freed(&hd),
                          "detail": "rush_2160.webm",
                          "note": "Refaite (plusieurs minutes) à la prochaine création 4K ; la 4K déjà créée reste.",
                          "keep": true}));
    }
    let copies = render_copies();
    if !copies.is_empty() {
        items.push(json!({"id": "rendu", "group": "videos", "label": "Copies des vidéos du dernier rendu",
                          "size": copies.iter().map(|p| freed(p)).sum::<u64>(),
                          "detail": copies.iter().map(|p| name_of(p)).collect::<Vec<_>>().join(", "),
                          "note": "Refaites au prochain rendu. Elles peuvent garder sur le disque une vidéo déjà supprimée ailleurs."}));
    }
    let parts = render_parts();
    if !parts.is_empty() {
        items.push(json!({"id": "parties", "group": "videos", "label": "Parties d'un rendu interrompu",
                          "size": parts.iter().map(|p| freed(p)).sum::<u64>(), "detail": format!("{} partie(s)", parts.len()),
                          "note": "Gardées pour reprendre le rendu sans les refaire ; refaites si elles sont effacées."}));
    }
    for name in util::list_dir(backups()) {
        items.push(json!({"id": format!("sauvegarde:{name}"), "group": "autres", "label": format!("Sauvegarde {name}"),
                          "size": freed(&backups().join(&name)),
                          "note": "Fichiers remplacés lors d'une réinitialisation d'Auto-montage."}));
    }
    let rest = leftovers();
    if !rest.is_empty() {
        items.push(json!({"id": "restes", "group": "autres", "label": "Envois coupés et fichiers inachevés",
                          "size": rest.iter().map(|p| freed(p)).sum::<u64>(),
                          "detail": rest.iter().map(|p| name_of(p)).collect::<Vec<_>>().join(", "),
                          "note": "Restes d'un envoi ou d'une tâche interrompus : inutilisables."}));
    }
    let free = crate::memoire::disk_free(paths::root()).unwrap_or(0);
    json!({"items": items, "free": free})
}

pub fn remove(path: &Path) {
    util::remove_all(path);
}

pub fn delete(ids: &[String]) -> Vec<String> {
    let inv = inventory();
    let known: Vec<&str> = util::arr(&inv, "items").iter().filter_map(|i| util::s(i, "id")).collect();
    let mut done = vec![];
    for item in ids {
        if !known.contains(&item.as_str()) {
            continue;
        }
        let (kind, name) = item.split_once(':').unwrap_or((item.as_str(), ""));
        let paths: Vec<PathBuf> = match kind {
            "projet" => vec![store().join(name)],
            "rush" => vec![paths::rushes().join(name)],
            "sauvegarde" => vec![backups().join(name)],
            "hd" => vec![paths::rushes().join("rush_2160.webm")],
            "rendu" => render_copies(),
            "parties" => render_parts(),
            "restes" => leftovers(),
            _ => vec![],
        };
        for p in paths {
            remove(&p);
        }
        done.push(item.clone());
    }
    done
}

pub fn main(args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("delete") {
        let before = crate::memoire::disk_free(paths::root()).unwrap_or(0);
        let done = delete(&args[1..]);
        let after = crate::memoire::disk_free(paths::root()).unwrap_or(0);
        println!("{}", pyjson::dumps(&json!({"deleted": done, "freed": after.saturating_sub(before)})));
    } else {
        println!("{}", pyjson::dumps_utf8(&inventory()));
    }
    0
}
