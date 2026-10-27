//! Dossiers du projet et programmes externes (ffmpeg, Whisper, Node).
//!
//! Racine (`ROOT`, le dossier `montage/` du dépôt) : `AM_ROOT` s'il est donné, sinon le premier
//! dossier parent du programme qui contient `selection/page.html` (programme compilé dans
//! `montage/rust/target/…` ou installé dans `montage/bin/`), sinon le dossier courant ou l'un de
//! ses parents.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Chemin absolu réel (liens résolus), comme `fs::canonicalize`, mais sans le préfixe `\\?\`
/// que Windows y ajoute : ffmpeg (listes concat), Node et git refusent ces chemins.
pub fn canon(p: &Path) -> std::io::Result<PathBuf> {
    let c = std::fs::canonicalize(p)?;
    Ok(strip_verbatim(c))
}

/// Retire le préfixe `\\?\` (ou `\\?\UNC\`) d'un chemin Windows.
pub fn strip_verbatim(p: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return p;
    }
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.len() >= 2 && rest.as_bytes()[1] == b':' => PathBuf::from(rest),
        _ => p,
    }
}

fn is_root(p: &Path) -> bool {
    p.join("selection").join("page.html").is_file()
}

pub static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Ok(r) = env::var("AM_ROOT") {
        return PathBuf::from(r);
    }
    let mut starts = vec![];
    if let Ok(exe) = env::current_exe() {
        starts.push(canon(&exe).unwrap_or(exe));
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

/// Ce programme, pour lancer ses autres commandes. Sous Linux, un programme recompilé (ou mis à
/// jour) pendant qu'il tourne est vu comme « … (deleted) » : on relance alors le nouveau fichier.
pub fn self_exe() -> PathBuf {
    let exe = env::current_exe().unwrap_or_else(|_| PathBuf::from("auto-montage"));
    match exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(path) if Path::new(path).exists() => PathBuf::from(path),
        _ => exe,
    }
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
    if let Ok(p) = env::var("AM_FFMPEG") {
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

/// Node.js autonome téléchargé par `auto-montage setup` (`montage/node/`) quand la machine n'a
/// pas de Node assez récent (20 ou plus).
pub fn node_home() -> PathBuf {
    root().join("node")
}

/// Dossier des programmes de ce Node (`node`, `npm`, `npx`).
pub fn node_home_bin() -> PathBuf {
    if cfg!(windows) {
        node_home()
    } else {
        node_home().join("bin")
    }
}

/// Node.js : celui de `montage/node/`, livré (`montage/bin/node`), ou celui du PATH.
pub fn node() -> PathBuf {
    let own = node_home_bin().join(exe_name("node"));
    if own.is_file() {
        return own;
    }
    bundled("node").or_else(|| which("node")).unwrap_or_else(|| PathBuf::from("node"))
}

/// Commande npm ou npx (`tool` = « npm » ou « npx ») : celle du Node de `montage/node/`
/// (lancée par ce node), sinon celle du PATH (`npm.cmd` sous Windows).
pub fn npm(tool: &str) -> std::process::Command {
    let cli = if cfg!(windows) { node_home().join("node_modules") } else { node_home().join("lib").join("node_modules") }
        .join("npm")
        .join("bin")
        .join(format!("{tool}-cli.js"));
    let own = node_home_bin().join(exe_name("node"));
    if own.is_file() && cli.is_file() {
        let mut c = std::process::Command::new(own);
        c.arg(cli);
        return c;
    }
    let mut found = None;
    if cfg!(windows) {
        found = ["cmd", "exe", "bat"].iter().find_map(|ext| which(&format!("{tool}.{ext}")));
    }
    std::process::Command::new(found.or_else(|| which(tool)).unwrap_or_else(|| PathBuf::from(tool)))
}

/// Dossier où `auto-montage setup` installe Claude Code quand il n'est pas sur la machine.
pub fn claude_home() -> PathBuf {
    root().join("claude")
}

/// Claude Code : celui de la machine (PATH), sinon celui installé par setup (`montage/claude/`).
pub fn claude() -> Option<PathBuf> {
    which("claude").or_else(|| {
        let bin = claude_home().join("node_modules").join(".bin");
        let names: &[&str] = if cfg!(windows) { &["claude.cmd", "claude.exe"] } else { &["claude"] };
        names.iter().map(|n| bin.join(n)).find(|p| p.is_file())
    })
}

/// Ajoute au PATH de ce programme (et des programmes qu'il lance : HyperFrames, Claude Code) les
/// programmes livrés : `montage/bin/` (ffmpeg, Whisper), Node de `montage/node/`, Claude Code de
/// `montage/claude/`.
pub fn extend_path() {
    let extra = [root().join("bin"), node_home_bin(), claude_home().join("node_modules").join(".bin")];
    let extra: Vec<PathBuf> = extra.into_iter().filter(|d| d.is_dir()).collect();
    if extra.is_empty() {
        return;
    }
    let current: Vec<PathBuf> = env::var_os("PATH").map(|p| env::split_paths(&p).collect()).unwrap_or_default();
    let mut all: Vec<PathBuf> = extra.into_iter().filter(|d| !current.contains(d)).collect();
    if all.is_empty() {
        return;
    }
    all.extend(current);
    if let Ok(joined) = env::join_paths(all) {
        // un seul fil d'exécution à ce moment (début de main)
        env::set_var("PATH", joined);
    }
}

/// Dossier de whisper.cpp (`WHISPER_DIR`, sinon `montage/whisper.cpp`).
pub fn whisper_dir() -> PathBuf {
    env::var("WHISPER_DIR").map(PathBuf::from).unwrap_or_else(|_| root().join("whisper.cpp"))
}

/// Programme de whisper.cpp (`whisper-cli`, `vad-speech-segments`) : compilé dans
/// `whisper.cpp/build/bin/` (`build/bin/Release/` sous Windows), sinon livré (`montage/bin/`), sinon le PATH.
pub fn whisper_bin(name: &str) -> PathBuf {
    let built = whisper_dir().join("build").join("bin").join(exe_name(name));
    if built.is_file() {
        return built;
    }
    // Windows (Visual Studio) : un dossier par configuration
    let release = whisper_dir().join("build").join("bin").join("Release").join(exe_name(name));
    if release.is_file() {
        return release;
    }
    bundled(name).or_else(|| which(name)).unwrap_or(built)
}

pub fn display(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
