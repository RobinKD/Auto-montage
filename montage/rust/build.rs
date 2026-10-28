//! Numéro de version du programme : celui du fichier VERSION du dépôt (le même que la page
//! « Mises à jour »), gardé dans le programme compilé (`auto-montage version`). La mise à jour
//! compare ce numéro à celui de la version installée pour savoir si le programme est le bon.
fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("VERSION");
    println!("cargo:rerun-if-changed={}", path.display());
    let v = std::fs::read_to_string(&path).map(|s| s.trim().to_string()).unwrap_or_else(|_| "0".into());
    println!("cargo:rustc-env=AM_VERSION={v}");
}
