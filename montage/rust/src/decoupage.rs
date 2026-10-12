//! (à porter : scripts/decoupage.py)

/// Refus d'une scission ou d'une fusion (message pour l'utilisateur).
#[derive(Debug, Clone)]
pub struct Refus(pub String);

impl std::fmt::Display for Refus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn merge(_mid: i64) -> Result<String, Refus> {
    Err(Refus("pas encore porté".into()))
}
pub fn unmerge(_mid: i64) -> Result<String, Refus> {
    Err(Refus("pas encore porté".into()))
}
pub fn split(_mid: i64, _t: f64) -> Result<String, Refus> {
    Err(Refus("pas encore porté".into()))
}
pub fn unsplit(_mid: i64) -> Result<String, Refus> {
    Err(Refus("pas encore porté".into()))
}

pub fn main(_args: &[String]) -> i32 {
    eprintln!("decoupage : pas encore porté en Rust");
    1
}
