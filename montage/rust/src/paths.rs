//! Dossiers du projet et programmes externes (ffmpeg, Whisper, Node).
//!
//! Racine (`ROOT`, le dossier `montage/` du dépôt) : `AM_ROOT` s'il est donné, sinon le premier
//! dossier parent du programme qui contient `selection/page.html` (programme compilé dans
//! `montage/rust/target/…` ou installé dans `montage/bin/`), sinon le dossier courant ou l'un de
//! ses parents.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

fn is_root(p: &Path) -> bool {
    p.join("selection").join("page.html").is_file()
}

pub static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Ok(r) = env::var("AM_ROOT") {
        return PathBuf::from(r);
    }
    let mut starts = vec![];
    if let Ok(exe) = env::current_exe() {
        starts.push(exe.canonicalize().unwrap_or(exe));
    }
    if let Ok(cwd) = env::current_dir() {
        starts.push(cwd.clone());
        starts.push(cwd.join("montage"));
    }
    for start in starts {
        let mut p: Option<&Path> = Some(start.as_path());
        while let Some(dir) = p {
            if is_root(dir) {
                return dir.to_path_buf();
            }
            p = dir.parent();
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
});

pub fn root() -> &'static Path {
    ROOT.as_path()
}
pub fn work() -> PathBuf {
    root().join("work")
}
pub fn public() -> PathBuf {
    root().join("public")
}
pub fn rushes() -> PathBuf {
    public().join("rushes")
}
pub fn out() -> PathBuf {
    root().join("out")
}
pub fn data() -> PathBuf {
    root().join("src").join("data")
}
pub fn scripts() -> PathBuf {
    root().join("scripts")
}

/// Ce programme (pour lancer ses propres sous-commandes).
pub fn self_exe() -> PathBuf {
    env::current_exe().unwrap_or_else(|_| PathBuf::from("auto-montage"))
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Programme trouvé dans le PATH.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for cand in [dir.join(exe_name(name)), dir.join(name)] {
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

fn bundled(name: &str) -> Option<PathBuf> {
    // Programmes livrés avec la version native : montage/bin/ ou à côté de ce programme.
    let mut dirs = vec![root().join("bin")];
    if let Some(d) = self_exe().parent() {
        dirs.push(d.to_path_buf());
    }
    dirs.into_iter().map(|d| d.join(exe_name(name))).find(|p| p.is_file())
}

/// ffmpeg : `AM_FFMPEG`, celui livré avec Auto-montage (`montage/bin/`), sinon celui du PATH.
pub static FFMPEG: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Ok(p) = env::var("AM_FFMPEG").or_else(|_| env::var("IMAGEIO_FFMPEG_EXE")) {
        return PathBuf::from(p);
    }
    bundled("ffmpeg").or_else(|| which("ffmpeg")).unwrap_or_else(|| PathBuf::from("ffmpeg"))
});

pub fn ffmpeg() -> &'static Path {
    FFMPEG.as_path()
}

/// ffprobe : `AM_FFPROBE`, livré, celui de @ffprobe-installer (paquets Node), sinon le PATH.
pub fn ffprobe() -> PathBuf {
    if let Ok(p) = env::var("AM_FFPROBE") {
        return PathBuf::from(p);
    }
    if let Some(p) = bundled("ffprobe") {
        return p;
    }
    let inst = root().join("node_modules").join("@ffprobe-installer");
    for d in crate::util::list_dir(&inst) {
        let p = inst.join(&d).join(exe_name("ffprobe"));
        if d != "ffprobe" && p.is_file() {
            return p;
        }
    }
    which("ffprobe").unwrap_or_else(|| PathBuf::from("ffprobe"))
}

/// Node.js : livré (`montage/bin/node`) ou celui du PATH.
pub fn node() -> PathBuf {
    bundled("node").or_else(|| which("node")).unwrap_or_else(|| PathBuf::from("node"))
}

/// Dossier de whisper.cpp (`WHISPER_DIR`, sinon `montage/whisper.cpp`).
pub fn whisper_dir() -> PathBuf {
    env::var("WHISPER_DIR").map(PathBuf::from).unwrap_or_else(|_| root().join("whisper.cpp"))
}

/// Programme de whisper.cpp (`whisper-cli`, `vad-speech-segments`) : compilé dans
/// `whisper.cpp/build/bin/`, sinon livré (`montage/bin/`), sinon le PATH.
pub fn whisper_bin(name: &str) -> PathBuf {
    let built = whisper_dir().join("build").join("bin").join(exe_name(name));
    if built.is_file() {
        return built;
    }
    bundled(name).or_else(|| which(name)).unwrap_or(built)
}

pub fn display(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
