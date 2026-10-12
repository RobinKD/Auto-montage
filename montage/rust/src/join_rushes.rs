//! (à porter : scripts/join_rushes.py)

/// Nom du rush assemblé à partir de plusieurs vidéos (« essai (assemblage de 2).mov »).
pub fn joined_name(names: &[String]) -> String {
    let first = std::path::Path::new(&names[0]);
    let stem: String = first.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default().chars().take(150).collect();
    format!("{stem} (assemblage de {}).mov", names.len())
}

pub fn main(_args: &[String]) -> i32 {
    eprintln!("join_rushes : pas encore porté en Rust");
    1
}
