//! Avancement des étapes longues, affiché en barre de progression par l'interface locale.
//!
//! Les étapes écrivent sur leur sortie des lignes « @progression <fait> <total> <libellé> » ;
//! l'interface locale (local_server) les lit (sans les mettre au journal) et les pages les
//! affichent. Dans un terminal, ces lignes restent lisibles telles quelles.
//!
//! En ligne de commande, un encodage ffmpeg suivi pas à pas (durée de l'entrée lue d'abord) :
//!   auto-montage progress ffmpeg "<libellé>" <ffmpeg> <arguments…>

use crate::procs;
use crate::util::fmt_g;
use regex::Regex;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

static LAST: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Écrit l'avancement, au plus toutes les « every » secondes par libellé (et toujours à la fin).
pub fn report_every(label: &str, done: f64, total: f64, every: f64) {
    let now = Instant::now();
    let mut last = LAST.lock().unwrap();
    if done < total {
        if let Some(t) = last.get(label) {
            if now.duration_since(*t).as_secs_f64() < every {
                return;
            }
        }
    }
    last.insert(label.to_string(), now);
    println!("@progression {} {} {}", fmt_g(done, 6), fmt_g(total, 6), label);
    let _ = std::io::stdout().flush();
}

pub fn report(label: &str, done: f64, total: f64) {
    report_every(label, done, total, 0.5)
}

/// Écrit une ligne et vide la sortie tout de suite (le journal de l'interface la lit au fil de l'eau).
pub fn say(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

static DURATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Duration: (\d+):(\d+):([\d.]+)").unwrap());

/// Durée d'une vidéo ou d'un son lue dans la sortie de « ffmpeg -i » (0 si illisible).
pub fn duration_of(info: &str) -> f64 {
    DURATION
        .captures(info)
        .map(|c| c[1].parse::<f64>().unwrap_or(0.0) * 3600.0 + c[2].parse::<f64>().unwrap_or(0.0) * 60.0 + c[3].parse::<f64>().unwrap_or(0.0))
        .unwrap_or(0.0)
}

/// Lance ffmpeg avec -progress et rapporte le temps encodé sur la durée de l'entrée (ou sur
/// « total » secondes de sortie, pour une sortie plus courte que l'entrée). Un morceau d'un
/// travail plus long (offset, whole) : rapporté comme offset + temps encodé sur whole.
/// `cmd[0]` est ffmpeg. Renvoie le code de sortie (128 + n si tué par le signal n, comme le shell).
pub fn ffmpeg(label: &str, cmd: &[String], total: Option<f64>, offset: f64, whole: Option<f64>) -> i32 {
    let total = match total {
        Some(t) => t,
        None => {
            let input = cmd.iter().position(|a| a == "-i").and_then(|i| cmd.get(i + 1));
            match input {
                Some(i) if cmd.len() > 1 => duration_of(&procs::run(&cmd[0], ["-hide_banner", "-i", i]).2),
                _ => 0.0,
            }
        }
    };
    let whole = whole.filter(|w| *w != 0.0).unwrap_or(total);
    if total > 0.0 {
        report(label, offset, whole); // la barre (et son chronomètre) démarre avec ffmpeg
    }
    let mut args: Vec<&str> = vec!["-progress", "pipe:1", "-nostats"];
    args.extend(cmd[1..].iter().map(String::as_str));
    let mut child = match Command::new(&cmd[0]).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            say(&format!("ERREUR : ffmpeg ({label}) ne se lance pas : {e}"));
            return 127;
        }
    };
    // Messages de ffmpeg recopiés au fil de l'eau (journal) et gardés pour le bilan d'un échec.
    let tail = std::sync::Arc::new(Mutex::new(VecDeque::<String>::new()));
    let stderr = child.stderr.take().unwrap();
    let tail2 = tail.clone();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            let line = line.trim_end().to_string();
            say(&line);
            let mut t = tail2.lock().unwrap();
            if t.len() == 40 {
                t.pop_front();
            }
            t.push_back(line);
        }
    });
    let stdout = child.stdout.take().unwrap();
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let (key, value) = line.trim().split_once('=').unwrap_or((line.trim(), ""));
        if key == "out_time_us" && total > 0.0 && !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) {
            let t = value.parse::<f64>().unwrap_or(0.0) / 1e6;
            report(label, offset + t.min(total), whole);
        }
    }
    let code = procs::wait(&mut child);
    let _ = reader.join();
    if code == 0 && total > 0.0 {
        report(label, offset + total, whole);
    }
    if code != 0 {
        say(&format!("ERREUR : ffmpeg ({label}) s'est arrêté : {}", exit_reason(code, false)));
        say(&format!("   {}", resources(Path::new(cmd.last().map(String::as_str).unwrap_or(".")))));
        let t = tail.lock().unwrap();
        if t.is_empty() {
            say("   ffmpeg n'a écrit aucun message d'erreur.");
        } else {
            say("   Derniers messages de ffmpeg :");
            for l in t.iter() {
                say(&format!("   | {l}"));
            }
        }
    }
    if code < 0 {
        128 - code
    } else {
        code
    }
}

/// Explication d'un code de sortie : négatif (programme tué par un signal) ou, avec shell, de
/// 129 à 159 (étape dont une commande a été tuée) : arrêté par un signal.
pub fn exit_reason(code: i32, shell: bool) -> String {
    let sig = if code < 0 {
        -code
    } else if shell && 128 < code && code < 160 {
        code - 128
    } else {
        0
    };
    if sig == 0 {
        return format!("code {code}");
    }
    let name = procs::signal_name(sig);
    let why = match name.as_str() {
        "SIGKILL" => "tué de force, le plus souvent par manque de mémoire (Docker : Réglages > Resources)",
        "SIGTERM" => "arrêté (annulation, arrêt ou redémarrage du conteneur)",
        "SIGSEGV" => "plantage du programme",
        "SIGBUS" => "plantage (disque plein ou fichier modifié pendant la lecture ?)",
        "SIGINT" => "interrompu",
        "SIGHUP" => "terminal fermé",
        "SIGPIPE" => "sortie fermée",
        _ => "",
    };
    format!(
        "{}signal {name}{}",
        if code > 0 { format!("code {code}, ") } else { String::new() },
        if why.is_empty() { String::new() } else { format!(" : {why}") }
    )
}

/// Place libre sur le disque de « path » et mémoire disponible, pour le journal.
pub fn resources(path: &Path) -> String {
    let mut out = vec![];
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let folder = abs.parent().map(Path::to_path_buf).unwrap_or_else(|| ".".into());
    if let Some(free) = crate::memoire::disk_free(&folder) {
        out.push(format!("disque libre {:.1} Go", free as f64 / (1u64 << 30) as f64));
    }
    if let (Some(avail), Some(total)) = crate::memoire::meminfo() {
        out.push(format!(
            "mémoire disponible {:.1} Go sur {:.1} Go",
            avail as f64 / (1u64 << 30) as f64,
            total as f64 / (1u64 << 30) as f64
        ));
    }
    if out.is_empty() {
        "ressources inconnues".into()
    } else {
        out.join(", ")
    }
}

/// `auto-montage progress ffmpeg "<libellé>" <ffmpeg> <arguments…>`
pub fn main(args: &[String]) -> i32 {
    if args.len() < 3 || args[0] != "ffmpeg" {
        eprintln!("Usage : auto-montage progress ffmpeg \"<libellé>\" <ffmpeg> <arguments…>");
        return 2;
    }
    ffmpeg(&args[1], &args[2..], None, 0.0, None)
}
