//! Lancement d'Auto-montage sur la machine, sans Docker (raccourcis des installateurs) :
//!
//!   auto-montage start [page] [--port N] [--sans-navigateur]
//!       Déjà lancé : ouvre la page dans le navigateur (après l'avoir relancé si ses fichiers ont
//!       changé de version). Sinon : installe ce qui manque (`setup`, long la première fois),
//!       démarre l'interface (`local_server`) et ouvre la page quand elle répond. Le programme
//!       reste ouvert tant que l'interface tourne (fenêtre de terminal des raccourcis).
//!   auto-montage stop [--port N]
//!       Arrête l'interface (une préparation en cours est mise en pause).
//!
//! Tout ce qui s'affiche au démarrage est aussi écrit dans work/logs/demarrage.log (page
//! « Journal »). Après une mise à jour, le serveur se relance lui-même avec `AM_RELANCE=1` : pas
//! de nouvelle installation ni de nouvel onglet.

use crate::paths;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u16 = 8080;

/// Requête HTTP à l'interface locale, sans proxy (127.0.0.1) : (code, corps).
fn local(method: &str, port: u16, path: &str) -> Option<(u16, String)> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let body = if method == "POST" { "{}" } else { "" };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut resp = String::new();
    let _ = s.read_to_string(&mut resp);
    let code = resp.split_whitespace().nth(1)?.parse().ok()?;
    let body = resp.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
    Some((code, body))
}

/// Version de l'interface qui tourne sur ce port, si elle répond.
fn running_version(port: u16) -> Option<String> {
    let (code, body) = local("GET", port, "/api/version")?;
    if code != 200 {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    v.get("version").and_then(|v| v.as_str()).map(String::from)
}

fn disk_version() -> String {
    crate::update::current_version()
}

/// Ouvre l'adresse dans le navigateur par défaut.
pub fn open_browser(url: &str) {
    let ok = if cfg!(windows) {
        Command::new("cmd").args(["/C", "start", "", url]).status()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else {
        Command::new("xdg-open").arg(url).stdout(Stdio::null()).stderr(Stdio::null()).status()
    };
    if !ok.map(|s| s.success()).unwrap_or(false) {
        println!("Ouvrez {url} dans votre navigateur.");
    }
}

/// Journal du démarrage (work/logs/demarrage.log), gardé à côté au-delà de 5 Mo.
fn open_log() -> Option<fs::File> {
    let dir = paths::work().join("logs");
    paths_mkdirs(&dir);
    let log = dir.join("demarrage.log");
    if fs::metadata(&log).map(|m| m.len() > 5_000_000).unwrap_or(false) {
        let _ = fs::rename(&log, dir.join("demarrage.prec.log"));
    }
    fs::OpenOptions::new().create(true).append(true).open(log).ok()
}

fn paths_mkdirs(dir: &std::path::Path) {
    let _ = fs::create_dir_all(dir);
}

/// Affiche une ligne et l'écrit dans le journal.
fn both(log: &mut Option<fs::File>, line: &str) {
    println!("{line}");
    if let Some(f) = log {
        let _ = writeln!(f, "{line}");
    }
}

/// `auto-montage setup`, sortie affichée et écrite dans le journal.
fn setup(log: &mut Option<fs::File>) -> bool {
    let child = Command::new(paths::self_exe())
        .arg("setup")
        .current_dir(paths::root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        both(log, "Impossible de lancer l'installation.");
        return false;
    };
    let err = child.stderr.take().map(|e| {
        std::thread::spawn(move || {
            let mut lines = vec![];
            for line in BufReader::new(e).lines().map_while(Result::ok) {
                eprintln!("{line}");
                lines.push(line);
            }
            lines
        })
    });
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            both(log, &line);
        }
    }
    if let Some(lines) = err.and_then(|h| h.join().ok()) {
        if let Some(f) = log {
            for line in lines {
                let _ = writeln!(f, "{line}");
            }
        }
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

struct Options {
    port: u16,
    page: String,
    browser: bool,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options { port: std::env::var("AM_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(PORT), page: String::new(), browser: true };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                o.port = args.get(i).and_then(|p| p.parse().ok()).ok_or("--port demande un nombre")?;
            }
            "--sans-navigateur" | "--no-browser" => o.browser = false,
            a if a.starts_with('-') => return Err(format!("argument inconnu : {a}")),
            a => o.page = a.trim_start_matches('/').to_string(),
        }
        i += 1;
    }
    Ok(o)
}

pub fn start(args: &[String]) -> i32 {
    let o = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Usage : auto-montage start [page] [--port N] [--sans-navigateur]\nstart : erreur : {e}");
            return 2;
        }
    };
    let url = format!("http://localhost:{}/{}", o.port, o.page);
    let relance = std::env::var_os("AM_RELANCE").is_some();
    // Déjà lancé (autre fenêtre, ou lancé au démarrage de la session) : on l'ouvre.
    if !relance {
        if let Some(v) = running_version(o.port) {
            let disk = disk_version();
            if v != disk {
                println!("Auto-montage {v} tourne encore alors que la version {disk} est installée : relance…");
                let _ = local("POST", o.port, "/api/restart");
                let t0 = Instant::now();
                while t0.elapsed() < Duration::from_secs(300) && running_version(o.port).as_deref() != Some(disk.as_str()) {
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
            println!("Auto-montage est déjà lancé : {url}");
            if o.browser {
                open_browser(&url);
            }
            return 0;
        }
    }
    let mut log = open_log();
    let now = chrono::Local::now().format("%d/%m/%Y à %H:%M:%S");
    both(&mut log, &format!("== Démarrage le {now} — Auto-montage {}", disk_version()));
    if !relance {
        both(&mut log, "Préparation de l'environnement (la première fois : 10 à 20 min)…");
        if !setup(&mut log) {
            both(&mut log, "L'installation n'est pas complète (voir les messages ci-dessus) : elle sera reprise au prochain lancement.");
        }
        paths::extend_path(); // Node et Claude Code tout juste installés
    }
    drop(log);
    if o.browser && !relance {
        let port = o.port;
        std::thread::spawn(move || {
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(120) {
                if running_version(port).is_some() {
                    open_browser(&url);
                    return;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });
    }
    println!("Auto-montage tourne tant que cette fenêtre reste ouverte (ou jusqu'à « Arrêter Auto-montage »).");
    crate::local_server::main(&["--port".to_string(), o.port.to_string()])
}

pub fn stop(args: &[String]) -> i32 {
    let o = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Usage : auto-montage stop [--port N]\nstop : erreur : {e}");
            return 2;
        }
    };
    match local("POST", o.port, "/api/quit") {
        Some((202, _)) => {
            println!("Auto-montage s'arrête.");
            0
        }
        Some((code, body)) => {
            eprintln!("Auto-montage n'a pas pu être arrêté (code {code}) : {body}");
            1
        }
        None => {
            println!("Auto-montage n'est pas lancé.");
            0
        }
    }
}
