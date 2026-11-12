//! Vidéos envoyées enregistrées sous leur empreinte, avec leur nom dans un registre.
//!
//! Chaque vidéo envoyée par la page est écrite dans public/rushes/ sous l'empreinte SHA-256 de son
//! contenu (16 premiers caractères, extension d'origine : « 3f9c0a….mp4 ») : une autre vidéo du
//! même nom ne remplace jamais la première (son montage enregistré et sa 4K restent justes), et la
//! même vidéo envoyée deux fois n'est gardée qu'une fois. Le nom montré partout (pages, montages,
//! fichiers téléchargés) vient du registre public/rushes/noms.json (fichier -> nom), que
//! « Renommer » change sans toucher à la vidéo. Vidéo absente du registre (envoyée avant ce
//! registre, ou copiée à la main) : son nom de fichier sans l'extension.
//! Vidéo d'un dérush validé et assemblage de plusieurs vidéos : identifiant tiré de ce dont elles
//! sont faites, nom « <rush> (dérush) », « <vidéo 1> (assemblage de N) ».

use crate::{paths, util};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Registre fichier -> nom montré, à côté des vidéos.
pub const REGISTRY: &str = "noms.json";
/// Longueur des noms montrés (en caractères).
pub const NAME_MAX: usize = 150;

fn registry_path() -> PathBuf {
    paths::rushes().join(REGISTRY)
}

fn load() -> Map<String, Value> {
    match util::read_json(registry_path()) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

fn save(m: &Map<String, Value>) -> Result<(), String> {
    util::mkdirs(paths::rushes());
    util::write_json_atomic(registry_path(), &Value::Object(m.clone()), Some(1)).map_err(|e| format!("public/rushes/{REGISTRY} : {e}"))
}

/// Nom du fichier sans dossier ni extension.
pub fn stem(file: &str) -> String {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    Path::new(base).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| base.to_string())
}

/// Nom montré d'une vidéo de public/rushes (nom de fichier, ou chemin « public/rushes/… »).
pub fn display(file: &str) -> String {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    if base.is_empty() {
        return String::new();
    }
    load().get(base).and_then(Value::as_str).map(String::from).unwrap_or_else(|| stem(base))
}

/// Nom acceptable : sans caractère de contrôle ni barre oblique, espaces réduits, NAME_MAX au plus.
pub fn clean(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || c == '/' || c == '\\' { ' ' } else { c }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(NAME_MAX).collect::<String>().trim().to_string()
}

/// Nom utilisable dans un nom de fichier téléchargé (lettres, chiffres, espaces et ponctuation simple).
pub fn file_safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || " .()+,'&-_".contains(c) { c } else { '_' }).collect();
    let s = s.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if s.is_empty() {
        "rush".into()
    } else {
        s
    }
}

/// Nom qu'aucune autre vidéo ne montre : « essai », sinon « essai (2) », « essai (3) »…
fn unique(m: &Map<String, Value>, wanted: &str, file: &str) -> String {
    let taken = |n: &str| {
        let low = n.to_lowercase();
        m.iter().any(|(f, v)| f != file && paths::rushes().join(f).is_file() && v.as_str().map(|s| s.to_lowercase()) == Some(low.clone()))
            || util::list_dir(paths::rushes()).iter().any(|f| f != file && !m.contains_key(f) && stem(f).to_lowercase() == low && crate::place::is_video(f))
    };
    if !taken(wanted) {
        return wanted.to_string();
    }
    (2..).map(|k| format!("{wanted} ({k})")).find(|n| !taken(n)).unwrap()
}

/// Inscrit le nom d'une vidéo (rendu unique). Renvoie le nom retenu.
pub fn register(file: &str, wanted: &str) -> Result<String, String> {
    let wanted = clean(wanted);
    let wanted = if wanted.is_empty() { stem(file) } else { wanted };
    let mut m = load();
    let name = unique(&m, &wanted, file);
    m.insert(file.to_string(), Value::String(name.clone()));
    save(&m)?;
    Ok(name)
}

/// « Renommer » : nouveau nom montré d'une vidéo. Refusé s'il est vide ou déjà pris.
pub fn rename(file: &str, wanted: &str) -> Result<String, String> {
    let wanted = clean(wanted);
    if wanted.is_empty() {
        return Err("Donnez un nom à la vidéo.".into());
    }
    if !paths::rushes().join(file).is_file() {
        return Err("Vidéo introuvable dans les rushes.".into());
    }
    let mut m = load();
    if unique(&m, &wanted, file) != wanted {
        return Err(format!("Une autre vidéo s'appelle déjà « {wanted} »."));
    }
    m.insert(file.to_string(), Value::String(wanted.clone()));
    save(&m)?;
    Ok(wanted)
}

/// Fichier d'une vidéo d'après son nom de fichier ou son nom montré (sans tenir compte de la casse).
pub fn resolve(wanted: &str) -> Option<String> {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return None;
    }
    let base = wanted.rsplit(['/', '\\']).next().unwrap_or(wanted);
    if paths::rushes().join(base).is_file() {
        return Some(base.to_string());
    }
    let low = wanted.to_lowercase();
    util::list_dir(paths::rushes())
        .into_iter()
        .filter(|f| crate::place::is_video(f) && !f.starts_with('.') && !f.contains(".part."))
        .find(|f| display(f).to_lowercase() == low)
}

/// Retire une vidéo effacée du registre.
pub fn forget(file: &str) {
    let mut m = load();
    if m.remove(file).is_some() {
        let _ = save(&m);
    }
}

/// Empreinte SHA-256 en cours de calcul (pendant l'écriture d'un envoi).
pub struct Hasher(Sha256);

impl Hasher {
    pub fn new() -> Self {
        Hasher(Sha256::new())
    }
    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    /// Nom de fichier : 16 premiers caractères de l'empreinte et extension (en minuscules).
    pub fn file_name(self, ext: &str) -> String {
        let hex = format!("{:x}", self.0.finalize());
        format!("{}.{}", &hex[..16], ext.to_lowercase())
    }
}

/// Identifiant tiré d'un texte (assemblage : noms des vidéos, qui sont leurs empreintes).
pub fn id_of(text: &str, ext: &str) -> String {
    let mut h = Hasher::new();
    h.update(text.as_bytes());
    h.file_name(ext)
}

/// Extension d'un nom de fichier (sans le point), « mp4 » à défaut.
pub fn ext_of(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).filter(|e| !e.is_empty()).unwrap_or_else(|| "mp4".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms() {
        assert_eq!(stem("public/rushes/essai.mov"), "essai");
        assert_eq!(clean("  mon\trush / test  "), "mon rush test");
        assert_eq!(file_safe("Réunion : 12/11"), "Réunion _ 12_11");
        assert_eq!(id_of("a.mp4\nb.mp4", "MOV").len(), 20);
        assert!(id_of("a", "mov").ends_with(".mov"));
        assert_ne!(id_of("a.mp4\nb.mp4", "mov"), id_of("b.mp4\na.mp4", "mov"));
    }
}
