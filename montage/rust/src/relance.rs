//! Le programme installé (`montage/bin/auto-montage`) : remplacé par la mise à jour et la
//! réinitialisation, puis relancé par le serveur local.
//!
//! - Version publiée : le programme compilé pour ce système est joint à chaque version sur GitHub
//!   (`auto-montage-<système>`, `asset_name`, construit par .github/workflows/programme.yml).
//! - Sans programme publié pour ce système : recompilé depuis les sources (`montage/rust/`) si
//!   Rust (cargo) est installé.
//! - Dans le conteneur Docker (`AM_IN_CONTAINER`) : rien à faire ici, le conteneur redémarre et
//!   recompile au démarrage (docker/entrypoint.sh).
//!
//! Le nouveau programme est d'abord écrit à côté (`auto-montage.new`) et vérifié (il se lance
//! et donne le numéro de la version attendue) ; il ne prend la place de l'ancien qu'après.
//! Sous Windows, un programme en cours d'exécution ne peut pas être remplacé, mais il peut être
//! renommé : l'ancien devient `auto-montage.old.exe`, effacé au lancement suivant.

use crate::paths;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Le programme installé : `montage/bin/auto-montage` (`.exe` sous Windows).
pub fn installed() -> PathBuf {
    paths::root().join("bin").join(exe("auto-montage"))
}

fn staged_path() -> PathBuf {
    paths::root().join("bin").join(exe("auto-montage.new"))
}

fn old_path() -> PathBuf {
    paths::root().join("bin").join(exe("auto-montage.old"))
}

/// Dans le conteneur Docker d'Auto-montage.
pub fn in_container() -> bool {
    env::var_os("AM_IN_CONTAINER").is_some_and(|v| !v.is_empty())
}

/// Nom de ce système dans les fichiers joints aux versions : linux-x86_64, linux-aarch64,
/// macos-arm64, macos-x86_64, windows-x86_64.
pub fn system_name() -> Option<String> {
    let os = match env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match (os, env::consts::ARCH) {
        (_, "x86_64") => "x86_64",
        ("macos", "aarch64") => "arm64",
        ("linux", "aarch64") => "aarch64",
        _ => return None,
    };
    Some(format!("{os}-{arch}"))
}

/// Nom du programme publié pour ce système (fichier joint aux versions GitHub).
pub fn asset_name() -> Option<String> {
    system_name().map(|s| format!("auto-montage-{s}{}", if cfg!(windows) { ".exe" } else { "" }))
}

/// Numéro de version que donne un programme (`<programme> version`), s'il se lance.
pub fn version_of(program: &Path) -> Option<String> {
    let out = Command::new(program).arg("version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn make_executable(_: &Path) {}

/// Copie un programme (téléchargé ou compilé) à côté de celui installé et vérifie qu'il se
/// lance et qu'il est de la version attendue. Renvoie son chemin, prêt pour `commit`.
pub fn stage(from: &Path, expected: &str) -> Result<PathBuf, String> {
    let staged = staged_path();
    if let Some(dir) = staged.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    }
    if from != staged {
        fs::copy(from, &staged).map_err(|e| format!("{} : {e}", staged.display()))?;
    }
    make_executable(&staged);
    match version_of(&staged) {
        Some(v) if v == expected => Ok(staged),
        Some(v) => {
            let _ = fs::remove_file(&staged);
            Err(format!("Le programme reçu est la version {v} au lieu de {expected} : rien n'est remplacé."))
        }
        None => {
            let _ = fs::remove_file(&staged);
            Err("Le programme reçu ne se lance pas sur ce système : rien n'est remplacé.".into())
        }
    }
}

/// Met le programme vérifié par `stage` à la place de celui installé.
pub fn commit(staged: &Path) -> Result<(), String> {
    let dest = installed();
    if cfg!(windows) && dest.exists() {
        // programme en cours d'exécution : renommé, pas remplacé
        let _ = fs::remove_file(old_path());
        fs::rename(&dest, old_path()).map_err(|e| format!("{} : {e}", dest.display()))?;
    }
    fs::rename(staged, &dest).map_err(|e| format!("{} : {e}", dest.display()))
}

/// Programme installé identique à celui-ci (même contenu) ?
pub fn same_as_installed(program: &Path) -> bool {
    match (fs::read(program), fs::read(installed())) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Efface les restes d'un remplacement (ancien programme sous Windows, essai interrompu).
pub fn cleanup() {
    let _ = fs::remove_file(old_path());
    let _ = fs::remove_file(staged_path());
}

/// Sources du programme et Rust présents : on peut le recompiler ici.
pub fn can_build() -> bool {
    paths::root().join("rust").join("Cargo.toml").is_file() && paths::which("cargo").is_some()
}

/// Dossier où cargo écrit le programme compilé.
fn target_dir() -> PathBuf {
    env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| paths::root().join("rust").join("target"))
}

/// Recompile le programme depuis les sources (`cargo build --release`) ; renvoie son chemin.
pub fn build() -> Result<PathBuf, String> {
    let Some(cargo) = paths::which("cargo") else {
        return Err("Rust (cargo) n'est pas installé : https://rustup.rs".into());
    };
    let manifest = paths::root().join("rust").join("Cargo.toml");
    let st = Command::new(cargo)
        .args(["build", "--release", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .status()
        .map_err(|e| format!("cargo : {e}"))?;
    if !st.success() {
        return Err("La compilation du programme a échoué (voir le journal).".into());
    }
    Ok(target_dir().join("release").join(exe("auto-montage")))
}

/// Lancé depuis les sources compilées ici (rust/target), pas depuis bin/ ?
fn running_from_build() -> bool {
    let me = paths::self_exe();
    let target = fs::canonicalize(target_dir()).unwrap_or_else(|_| target_dir());
    fs::canonicalize(&me).unwrap_or(me).starts_with(target)
}

/// Relance le serveur avec le programme à jour, mêmes arguments : après une mise à jour, une
/// réinitialisation, ou des fichiers changés sur le disque. Dans le conteneur : simple arrêt (le
/// conteneur redémarre et recompile). Programme lancé depuis les sources : recompilé d'abord.
pub fn relaunch() -> ! {
    if in_container() {
        std::process::exit(0);
    }
    let program = if running_from_build() && can_build() {
        match build() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{e} Relance avec le programme actuel.");
                paths::self_exe()
            }
        }
    } else if installed().is_file() {
        installed()
    } else {
        paths::self_exe()
    };
    let args: Vec<std::ffi::OsString> = env::args_os().skip(1).collect();
    eprintln!("Relance d'Auto-montage ({})…", program.display());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // même processus (le lanceur et le terminal le suivent) ; le port est libéré par exec
        let err = Command::new(&program).args(&args).env("AM_RELANCE", "1").exec();
        eprintln!("Relance impossible : {err}");
        std::process::exit(1);
    }
    #[cfg(not(unix))]
    {
        // nouveau processus dans la même console ; il attend que ce serveur libère le port
        match Command::new(&program).args(&args).env("AM_RELANCE", "1").spawn() {
            Ok(_) => std::process::exit(0),
            Err(e) => {
                eprintln!("Relance impossible : {e}");
                std::process::exit(1);
            }
        }
    }
}
