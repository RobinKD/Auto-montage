//! Installation de ce dont Auto-montage a besoin (remplace setup.sh, setup_whisper.sh,
//! fetch_fonts.sh, fetch_face_model.sh, ensure_browser.sh et check_network.sh). Chaque commande
//! saute ce qui existe déjà. Téléchargements par `net` (proxy de l'environnement, certificats du
//! système) au lieu de curl ; archive de la police ouverte par le crate zip. npm, git, cmake et la
//! commande hyperframes restent des programmes externes.
//!
//! - `auto-montage setup` : tout (paquets Node, navigateur de rendu, Whisper, police, modèles des
//!   visages). Appelé par le hook de session Claude Code sur le web et par `auto-montage start`.
//! - `setup_whisper` : whisper.cpp compilé pour le CPU (sauf s'il est déjà là, compilé ou livré
//!   dans `montage/bin/` ; version publiée : téléchargé déjà compilé) et ses modèles.
//! - `fetch_node` : Node.js autonome dans `montage/node/` si la machine n'a pas Node 20 ou plus.
//! - `claude_code` : Claude Code dans `montage/claude/` s'il manque, et le plugin HyperFrames.
//! - `fetch_fonts` : police Oliver (usage personnel, non versionnée).
//! - `fetch_face_model` : modèles des visages YuNet et SFace.
//! - `ensure_browser` : navigateur de rendu de HyperFrames, chemin affiché en dernière ligne.
//! - `check_network` : domaines de env/allowed-domains.txt joignables.

use crate::{net, paths, procs, util};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const WHISPER_VERSION: &str = "1.7.6";
const PLAYWRIGHT_VERSION: &str = "1.56.1";

/// Programme externe : `name` du PATH (`name.cmd` sous Windows pour npm et npx).
fn tool(name: &str) -> PathBuf {
    if cfg!(windows) {
        for ext in ["cmd", "exe", "bat"] {
            if let Some(p) = paths::which(&format!("{name}.{ext}")) {
                return p;
            }
        }
    }
    paths::which(name).unwrap_or_else(|| PathBuf::from(name))
}

/// Lance une commande depuis la racine du projet, sortie héritée ; renvoie son code.
fn call_in(dir: &Path, program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> i32 {
    match Command::new(program.as_ref()).args(args).current_dir(dir).status() {
        Ok(s) => procs::py_code(&s),
        Err(e) => {
            eprintln!("Impossible de lancer {} : {e}", program.as_ref().to_string_lossy());
            127
        }
    }
}

/// Une sous-commande de ce programme (ex. « setup_whisper »), sortie héritée.
fn sub(name: &str) -> i32 {
    call_in(paths::root(), paths::self_exe(), &[name])
}

/// Télécharge `url` dans `out` au fil de l'eau (gros modèles : pas tout en mémoire), sous un nom
/// provisoire `.part` renommé à la fin, avec `retries` nouvelles tentatives (comme curl --retry).
fn download_file(url: &str, out: &Path, retries: u32) -> Result<u64, String> {
    let part = out.with_file_name(format!("{}.part", out.file_name().unwrap().to_string_lossy()));
    let mut last = String::new();
    for attempt in 0..=retries {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(1 << attempt.min(4)));
        }
        let resp = match net::agent(Duration::from_secs(3600)).get(url).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => {
                last = format!("HTTP {code} pour {url}");
                if code < 500 && code != 429 {
                    break;
                }
                continue;
            }
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        let result = (|| -> std::io::Result<u64> {
            let mut f = fs::File::create(&part)?;
            let mut reader = resp.into_reader();
            let mut buf = vec![0u8; 1 << 16];
            let mut total = 0u64;
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                f.write_all(&buf[..n])?;
                total += n as u64;
            }
            f.flush()?;
            Ok(total)
        })();
        match result {
            Ok(total) => {
                fs::rename(&part, out).map_err(|e| e.to_string())?;
                return Ok(total);
            }
            Err(e) => {
                last = e.to_string();
                let _ = fs::remove_file(&part);
            }
        }
    }
    let _ = fs::remove_file(&part);
    Err(last)
}

/// `auto-montage setup` : installe les dépendances du projet. Idempotent : chaque étape saute ce
/// qui existe déjà. Appelé par le hook de session Claude Code sur le web
/// (.claude/hooks/session-start.sh) et par `auto-montage start` (lanceur.rs).
pub fn main(_args: &[String]) -> i32 {
    let root = paths::root();
    // Node.js autonome si la machine n'en a pas (Node 20 ou plus).
    let code = sub("fetch_node");
    if code != 0 {
        return code;
    }
    // Paquets Node (HyperFrames, GSAP).
    let stamp = root.join("node_modules").join(".package-lock.json");
    let newer = match (util::mtime(root.join("package.json")), util::mtime(&stamp)) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        _ => false,
    };
    if !root.join("node_modules").join("hyperframes").is_dir() || newer {
        let code = match paths::npm("npm").args(["install", "--no-audit", "--no-fund"]).current_dir(root).status() {
            Ok(s) => procs::py_code(&s),
            Err(e) => {
                eprintln!("Impossible de lancer npm : {e}");
                127
            }
        };
        if code != 0 {
            return code;
        }
    }

    // ffmpeg livré avec Auto-montage (celui d'imageio-ffmpeg).
    if sub("fetch_ffmpeg") != 0 {
        println!("ffmpeg non installé (réessayé au prochain démarrage) : celui de la machine sera utilisé.");
    }

    // Navigateur de rendu de HyperFrames (Chromium de Playwright sur Linux ARM64).
    let browser = Command::new(paths::self_exe()).arg("ensure_browser").current_dir(root).stdout(Stdio::null()).status();
    if !browser.map(|s| s.success()).unwrap_or(false) {
        println!("Navigateur de rendu non installé (réessayé au prochain démarrage et à chaque rendu).");
    }

    // Whisper compilé pour le CPU + modèles (environ 1,7 Go en tout, seulement la première fois).
    let code = sub("setup_whisper");
    if code != 0 {
        return code;
    }

    // Police Oliver (usage personnel, non versionnée).
    let code = sub("fetch_fonts");
    if code != 0 {
        return code;
    }

    // Modèles de détection et de reconnaissance des visages (cadrage des zooms).
    let _ = sub("fetch_face_model");

    // Claude Code (mode Auto-montage : consignes, discussion, analyse du style) et son plugin.
    if sub("claude_code") != 0 {
        println!("Claude Code non installé (réessayé au prochain démarrage) : le mode Dérushage marche sans lui.");
    }

    println!("Environnement Auto-montage prêt.");
    0
}

/// Installe whisper.cpp (version publiée compilée pour le CPU, ou compilé ici : avec CUDA ou Vulkan
/// si leur boîte à outils est installée, `gpu::whisper_flag`) et télécharge les modèles utilisés par
/// transcribe_segments et prepare. Ne refait rien si tout est déjà là ; des programmes déjà
/// compilés trouvés par `paths::whisper_bin` (montage/bin/, PATH) suffisent.
pub fn setup_whisper(_args: &[String]) -> i32 {
    // WHISPER_DIR : autre emplacement de whisper.cpp (par défaut montage/whisper.cpp).
    let whisper = paths::whisper_dir();
    let ready = || ["whisper-cli", "vad-speech-segments"].iter().all(|n| paths::whisper_bin(n).is_file());
    if !ready() {
        // Version publiée : whisper.cpp déjà compilé pour ce système, joint à la version
        // (whisper-<système>.tar.gz, .github/workflows/programme.yml) : rien à compiler ici.
        if let Err(e) = fetch_whisper() {
            eprintln!("{e}");
        }
    }
    if !ready() {
        if !whisper.join(".git").is_dir() {
            util::remove_all(&whisper);
            let code = procs::call(
                tool("git"),
                [
                    "clone",
                    "--quiet",
                    "--depth",
                    "1",
                    "--branch",
                    &format!("v{WHISPER_VERSION}"),
                    "https://github.com/ggml-org/whisper.cpp.git",
                    &paths::display(&whisper),
                ],
            );
            if code != 0 {
                eprintln!("Échec du téléchargement de whisper.cpp (git clone, code {code})");
                return code;
            }
        }
        // Même compilation que « make » de whisper.cpp (CMake dans build/, binaires dans build/bin/).
        let jobs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).to_string();
        let cmake = tool("cmake");
        let build = |flag: Option<&str>| -> i32 {
            let mut code = call_in(&whisper, &cmake, &[&["-B", "build"][..], flag.as_slice()].concat());
            if code == 0 {
                code = call_in(&whisper, &cmake, &["--build", "build", "--config", "Release", "-j", &jobs]);
            }
            code
        };
        // Calcul par la carte graphique (CUDA, Vulkan) si sa boîte à outils est là (gpu.rs,
        // AM_GPU=0 : non) ; échec : compilation pour le processeur seul.
        let mut code = match crate::gpu::whisper_flag() {
            Some((flag, name)) => {
                println!("Compilation de whisper.cpp avec {name} (carte graphique)");
                let code = build(Some(flag));
                if code != 0 {
                    println!("Échec de la compilation avec {name} : compilation pour le processeur");
                    util::remove_all(whisper.join("build"));
                }
                code
            }
            None => 1,
        };
        if code != 0 {
            code = build(None);
        }
        if code != 0 {
            eprintln!("Échec de la compilation de whisper.cpp (cmake, code {code})");
            return code;
        }
    }

    // Téléchargés par net (passe par le proxy de l'environnement s'il y en a un).
    util::mkdirs(&whisper);
    for (name, url) in [
        ("ggml-large-v3-turbo.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin"),
        ("ggml-silero-v5.1.2.bin", "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin"),
    ] {
        let out = whisper.join(name);
        if out.is_file() {
            continue;
        }
        if let Err(e) = download_file(url, &out, 4) {
            eprintln!("Échec du téléchargement de {name} : {e}");
            return 1;
        }
    }
    println!("whisper.cpp prêt dans {}", paths::display(&whisper));
    0
}

/// Programmes de whisper.cpp compilés pour ce système, joints à la version installée sur GitHub
/// (`whisper-<système>.tar.gz`), extraits dans `montage/bin/`. Ok(()) aussi quand la version n'en
/// a pas (version de travail) : setup_whisper les compile alors.
fn fetch_whisper() -> Result<(), String> {
    let Some(system) = crate::relance::system_name() else { return Ok(()) };
    let name = format!("whisper-{system}.tar.gz");
    let tag = format!("v{}", crate::update::current_version());
    let Some(url) = crate::update::release_asset(&tag, &name)? else { return Ok(()) };
    println!("Téléchargement de whisper.cpp compilé pour ce système ({name})…");
    let bin = paths::root().join("bin");
    util::mkdirs(&bin);
    let archive = bin.join(format!("{name}.dl"));
    download_file(&url, &archive, 4).map_err(|e| format!("whisper.cpp : {e}"))?;
    let result = (|| -> Result<(), String> {
        let f = fs::File::open(&archive).map_err(|e| e.to_string())?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(f));
        for entry in tar.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path().map_err(|e| e.to_string())?.into_owned();
            // fichiers seuls, sans dossier : rien hors de bin/
            let Some(file) = path.file_name().map(PathBuf::from) else { continue };
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let out = bin.join(&file);
            entry.unpack(&out).map_err(|e| format!("{} : {e}", out.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(0o755));
            }
        }
        Ok(())
    })();
    let _ = fs::remove_file(&archive);
    result
}

/// Version de Node.js téléchargée quand la machine n'en a pas (ou une trop ancienne).
const NODE_VERSION: &str = "22.12.0";

/// Version majeure du Node du PATH (`node --version` : « v22.12.0 »).
fn system_node_major() -> Option<u32> {
    let node = paths::which(if cfg!(windows) { "node.exe" } else { "node" })?;
    let out = Command::new(node).arg("--version").output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout);
    v.trim().trim_start_matches('v').split('.').next()?.parse().ok()
}

/// Node.js autonome dans `montage/node/` (HyperFrames, Claude Code), depuis nodejs.org, si la
/// machine n'a pas Node 20 ou plus récent.
pub fn fetch_node(_args: &[String]) -> i32 {
    let own = paths::node_home_bin().join(if cfg!(windows) { "node.exe" } else { "node" });
    if own.is_file() || system_node_major().is_some_and(|m| m >= 20) {
        return 0;
    }
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "win",
        other => {
            eprintln!("Node.js : système non pris en charge ({other}), installez Node 22 : https://nodejs.org");
            return 1;
        }
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => {
            eprintln!("Node.js : processeur non pris en charge ({other}), installez Node 22 : https://nodejs.org");
            return 1;
        }
    };
    let ext = if os == "win" { "zip" } else { "tar.gz" };
    let base = format!("node-v{NODE_VERSION}-{os}-{arch}");
    let url = format!("https://nodejs.org/dist/v{NODE_VERSION}/{base}.{ext}");
    println!("Téléchargement de Node.js {NODE_VERSION} ({base})…");
    let home = paths::node_home();
    let tmp = paths::root().join(format!("node.{ext}.dl"));
    if let Err(e) = download_file(&url, &tmp, 4) {
        eprintln!("Node.js : téléchargement impossible ({e})");
        return 1;
    }
    let staging = paths::root().join("node.installation");
    util::remove_all(&staging);
    let result = (|| -> Result<(), String> {
        let f = fs::File::open(&tmp).map_err(|e| e.to_string())?;
        if ext == "zip" {
            zip::ZipArchive::new(f).and_then(|mut z| z.extract(&staging)).map_err(|e| e.to_string())?;
        } else {
            tar::Archive::new(flate2::read::GzDecoder::new(f)).unpack(&staging).map_err(|e| e.to_string())?;
        }
        util::remove_all(&home);
        fs::rename(staging.join(&base), &home).map_err(|e| e.to_string())
    })();
    let _ = fs::remove_file(&tmp);
    util::remove_all(&staging);
    match result {
        Ok(()) => {
            println!("Node.js installé dans {}", paths::display(&home));
            0
        }
        Err(e) => {
            eprintln!("Node.js : archive illisible ({e})");
            1
        }
    }
}

/// Claude Code (dans `montage/claude/` s'il n'est pas sur la machine) et le plugin HyperFrames du
/// projet, installé une fois par utilisateur.
pub fn claude_code(_args: &[String]) -> i32 {
    if paths::claude().is_none() {
        println!("Installation de Claude Code…");
        let home = paths::claude_home();
        util::mkdirs(&home);
        let st = paths::npm("npm")
            .args(["install", "--no-audit", "--no-fund", "--prefix"])
            .arg(&home)
            .arg("@anthropic-ai/claude-code")
            .current_dir(paths::root())
            .status();
        if !st.map(|s| s.success()).unwrap_or(false) || paths::claude().is_none() {
            eprintln!("Claude Code : installation impossible (npm).");
            return 1;
        }
    }
    let Some(claude) = paths::claude() else { return 1 };
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let Some(flag) = home.map(|h| h.join(".auto-montage-plugins-2")) else { return 0 };
    if flag.exists() {
        return 0;
    }
    let quiet = |args: &[&str]| {
        Command::new(&claude).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
    };
    if quiet(&["plugin", "marketplace", "add", "heygen-com/hyperframes"]) && quiet(&["plugin", "install", "hyperframes@hyperframes"]) {
        let _ = fs::write(&flag, "");
    } else {
        println!("Plugin HyperFrames de Claude Code non installé (réessayé au prochain démarrage).");
    }
    0
}

/// Télécharge la police Oliver (Iva Florina, dafont) dans public/fonts/.
/// Licence : gratuite pour un usage personnel uniquement ; usage commercial soumis à licence
/// auprès de l'autrice. Elle n'est donc pas versionnée.
pub fn fetch_fonts(_args: &[String]) -> i32 {
    let fonts = paths::public().join("fonts");
    let out = fonts.join("Oliver-Regular.ttf");
    if out.is_file() {
        return 0;
    }
    let body = match net::get("https://dl.dafont.com/dl/?f=oliver_3", &[], Duration::from_secs(120)) {
        Ok((code, body)) if (200..300).contains(&code) => body,
        Ok((code, _)) => {
            eprintln!("Police Oliver : téléchargement refusé (HTTP {code})");
            return 1;
        }
        Err(e) => {
            eprintln!("Police Oliver : téléchargement impossible ({e})");
            return 1;
        }
    };
    let result = (|| -> Result<(), String> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(body)).map_err(|e| e.to_string())?;
        let mut file = zip.by_name("Oliver-Regular.ttf").map_err(|e| e.to_string())?;
        let mut data = vec![];
        file.read_to_end(&mut data).map_err(|e| e.to_string())?;
        util::mkdirs(&fonts);
        let part = fonts.join("Oliver-Regular.ttf.part");
        fs::write(&part, data).map_err(|e| e.to_string())?;
        fs::rename(&part, &out).map_err(|e| e.to_string())
    })();
    if let Err(e) = result {
        eprintln!("Police Oliver : archive illisible ({e})");
        return 1;
    }
    println!("Oliver installée dans public/fonts/");
    0
}

/// Modèles des visages d'opencv_zoo, copies de Hugging Face (detect_face) : YuNet (détection,
/// 230 Ko, licence MIT) et SFace (qui est qui, 37 Mo, licence Apache 2.0).
pub fn fetch_face_model(_args: &[String]) -> i32 {
    let models = paths::work().join("models");
    util::mkdirs(&models);
    let fetch = |file: &str, repo: &str| -> bool {
        let out = models.join(file);
        if fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false) {
            return true;
        }
        let url = format!("https://huggingface.co/opencv/{repo}/resolve/main/{file}");
        let part = models.join(format!("{file}.dl"));
        match download_file(&url, &part, 0) {
            // Moins de 100 Ko : page d'erreur ou pointeur Git LFS, pas le modèle.
            Ok(n) if n > 100_000 => fs::rename(&part, &out).is_ok(),
            Ok(_) => {
                let _ = fs::remove_file(&part);
                eprintln!("Modèle {file} : téléchargement invalide");
                false
            }
            Err(e) => {
                let _ = fs::remove_file(&part);
                eprintln!("Modèle {file} : {e}");
                false
            }
        }
    };
    if !fetch("face_detection_yunet_2023mar.onnx", "face_detection_yunet") {
        return 1;
    }
    if !fetch("face_recognition_sface_2021dec.onnx", "face_recognition_sface") {
        eprintln!("SFace indisponible : personnes non reconnues d'un plan à l'autre.");
    }
    0
}

/// Chrome sans écran de Playwright déjà présent (environnement cloud : /opt/pw-browsers ; Linux
/// ARM64 : ~/.cache/ms-playwright).
fn find_shell(pw_dir: &Path) -> Option<PathBuf> {
    for base in [PathBuf::from("/opt/pw-browsers"), pw_dir.to_path_buf()] {
        for d in util::list_dir(&base) {
            if d.starts_with("chromium_headless_shell-") {
                let p = base.join(&d).join("chrome-linux").join("headless_shell");
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Navigateur de rendu de HyperFrames (Chrome sans écran) : affiche son chemin en dernière
/// ligne, après l'avoir téléchargé s'il manque. Appelé par setup et render_lib::headless_chrome.
/// - Environnement cloud : le Chromium fourni (/opt/pw-browsers), sans téléchargement.
/// - Linux ARM64 (Raspberry Pi, machines virtuelles Linux sur un Mac Apple Silicon…) : Google ne
///   publie pas de « chrome-headless-shell » pour cette plateforme, et HyperFrames essaie alors un
///   apt-get qui demande les droits d'administrateur. On prend celui de Playwright (même
///   Chrome 141 que l'environnement cloud).
/// - Ailleurs : « hyperframes browser ensure » (Chrome téléchargé dans ~/.cache/hyperframes).
pub fn ensure_browser(_args: &[String]) -> i32 {
    let root = paths::root();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let pw_dir = home.join(".cache").join("ms-playwright");
    if cfg!(target_os = "linux") {
        if let Some(p) = find_shell(&pw_dir) {
            println!("{}", paths::display(&p));
            return 0;
        }
        if cfg!(target_arch = "aarch64") {
            eprintln!("Navigateur de rendu pour Linux ARM64 : téléchargement du Chromium de Playwright…");
            let status = paths::npm("npx")
                .args(["-y", &format!("playwright@{PLAYWRIGHT_VERSION}"), "install", "--only-shell", "chromium"])
                .env("PLAYWRIGHT_BROWSERS_PATH", &pw_dir)
                .env("PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD", "")
                .current_dir(root)
                .stdout(Stdio::from(std::io::stderr()))
                .status();
            if !status.map(|s| s.success()).unwrap_or(false) {
                return 1;
            }
            if let Some(p) = find_shell(&pw_dir) {
                println!("{}", paths::display(&p));
                return 0;
            }
            eprintln!("Chromium de Playwright introuvable après le téléchargement ({}).", paths::display(&pw_dir));
            return 1;
        }
    }
    let cli = root.join("node_modules").join("hyperframes").join("dist").join("cli.js");
    match Command::new(paths::node()).arg(&cli).args(["browser", "ensure"]).env("HYPERFRAMES_NO_TELEMETRY", "1").current_dir(root).status() {
        Ok(s) => procs::py_code(&s),
        Err(e) => {
            eprintln!("Impossible de lancer node : {e}");
            127
        }
    }
}

/// Vérifie que les domaines de env/allowed-domains.txt sont joignables.
/// Code de sortie 1 si au moins un domaine est bloqué par l'environnement.
pub fn check_network(_args: &[String]) -> i32 {
    let list = fs::read_to_string(paths::root().join("env").join("allowed-domains.txt")).unwrap_or_default();
    let mut blocked = vec![];
    for line in list.lines() {
        let mut cols = line.split_whitespace();
        let (Some(domain), url) = (cols.next(), cols.next()) else { continue };
        if domain.starts_with('#') {
            continue;
        }
        // Une réponse HTTP, quelle qu'elle soit, prouve que le domaine est joignable.
        let ok = url.is_some_and(|u| net::get(u, &[], Duration::from_secs(10)).is_ok());
        if ok {
            println!("ok      {domain}");
        } else {
            println!("BLOQUÉ  {domain}");
            blocked.push(domain.to_string());
        }
    }
    if !blocked.is_empty() {
        println!();
        println!("À ajouter dans Network access de l'environnement : {}", blocked.join(" "));
        return 1;
    }
    0
}

/// Version d'imageio-ffmpeg dont le ffmpeg est repris.
const IMAGEIO_FFMPEG: &str = "0.6.0";

/// ffmpeg livré avec Auto-montage (montage/bin/ffmpeg) : le ffmpeg statique que le paquet Python
/// imageio-ffmpeg fournit pour chaque système (avec les filtres des voix modifiées : rubberband…), pris dans son archive sur PyPI. Rien à faire s'il est
/// déjà là ou si AM_FFMPEG en désigne un autre ; système sans archive : celui du PATH.
pub fn fetch_ffmpeg(_args: &[String]) -> i32 {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let bin = paths::root().join("bin");
    let target = bin.join(exe);
    if target.is_file() || std::env::var_os("AM_FFMPEG").is_some() {
        return 0;
    }
    let tag = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "manylinux2014_x86_64",
        ("linux", "aarch64") => "manylinux2014_aarch64",
        ("macos", "aarch64") => "macosx_11_0_arm64",
        ("macos", "x86_64") => "macosx_10_9_x86_64",
        ("windows", "x86_64") => "win_amd64",
        ("windows", "x86") => "win32",
        _ => {
            println!("Pas de ffmpeg fourni pour ce système : celui de la machine sera utilisé.");
            return 0;
        }
    };
    let api = format!("https://pypi.org/pypi/imageio-ffmpeg/{IMAGEIO_FFMPEG}/json");
    let info: serde_json::Value = match net::get(&api, &[], Duration::from_secs(60)) {
        Ok((200, body)) => serde_json::from_slice(&body).unwrap_or_default(),
        Ok((code, _)) => {
            eprintln!("ffmpeg : PyPI répond {code} ; celui de la machine sera utilisé s'il existe.");
            return 1;
        }
        Err(e) => {
            eprintln!("ffmpeg : pas de connexion à PyPI ({e}) ; celui de la machine sera utilisé s'il existe.");
            return 1;
        }
    };
    let Some(url) = util::arr(&info, "urls")
        .iter()
        .find(|u| util::s(u, "filename").map(|f| f.ends_with(".whl") && f.contains(tag)).unwrap_or(false))
        .and_then(|u| util::s(u, "url"))
    else {
        eprintln!("ffmpeg : archive introuvable pour ce système ({tag}).");
        return 1;
    };
    println!("Téléchargement de ffmpeg ({tag})…");
    util::mkdirs(&bin);
    let wheel = bin.join(".ffmpeg.whl");
    if let Err(e) = net::download(url, &wheel, Duration::from_secs(600)) {
        eprintln!("ffmpeg : téléchargement impossible ({e}).");
        return 1;
    }
    let result = (|| -> Result<(), String> {
        let mut zip = zip::ZipArchive::new(fs::File::open(&wheel).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let name = zip
            .file_names()
            .find(|n| n.starts_with("imageio_ffmpeg/binaries/ffmpeg-"))
            .map(String::from)
            .ok_or("ffmpeg absent de l'archive")?;
        let mut entry = zip.by_name(&name).map_err(|e| e.to_string())?;
        let part = bin.join(format!("{exe}.part"));
        let mut out = fs::File::create(&part).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        drop(out);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&part, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
        fs::rename(&part, &target).map_err(|e| e.to_string())
    })();
    let _ = fs::remove_file(&wheel);
    match result {
        Ok(()) => {
            println!("ffmpeg installé dans {}", target.display());
            0
        }
        Err(e) => {
            eprintln!("ffmpeg : {e}");
            1
        }
    }
}
