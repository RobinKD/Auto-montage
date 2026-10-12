//! Mémoire nécessaire à chaque étape, d'après la vidéo, comparée à la mémoire disponible.
//!
//! Chaque opération a un besoin estimé (pic de mémoire de tous ses programmes) qui dépend de la
//! taille de l'image, de la durée du rush ou du montage et du nombre de navigateurs du rendu.
//! Les étapes se suivent : le besoin d'une tâche est celui de son opération la plus gourmande.
//! Mesures (6 octobre 2026, 4 cœurs, pic de la somme des PSS de tous les processus) : version de
//! travail 185 à 410 Mo (1080p, 4K vers 1080p, 9:16), transcription 2,1 Go (Whisper large-v3-turbo,
//! modèle de 1,6 Go), visage 120 Mo, montage (build_edit) 150 Mo pour 2 min de rush (son du rush
//! entier en mémoire). Rendu HyperFrames 1080p (7 octobre, programmes lancés compris, navigateurs
//! dans leur propre session) avec 4 navigateurs : 4,6 Go pour 80 s de montage en 8 plans, 6 Go pour
//! 5 min en 8 plans, 7 Go pour 5 min en 150 plans (navigateurs de 1,05 à 1,3 Go chacun). Rendu 4K :
//! 4 navigateurs ont dépassé 14 Go. Le rendu prend autant de navigateurs (un par cœur au plus) que
//! 80 % de la mémoire libre le permet (choose_workers).
//!
//! Expérience de la machine : l'interface locale mesure le vrai pic de chaque opération pendant les
//! préparations (somme des PSS des programmes de l'étape) et le garde dans work/memoire_mesures.json
//! avec le besoin prévu pour ce rush-là (record). Le besoin prévu d'un nouveau rush est celui de la
//! formule (qui suit la taille de l'image et la durée), multiplié par le plus grand rapport mesuré /
//! prévu des 5 dernières mesures de l'opération, plus 10 % de marge (entre 0,5 et 3 fois la formule).
//!
//! Mémoire disponible : MemAvailable de /proc/meminfo, bornée par la limite du conteneur (cgroup)
//! quand il y en a une ; hors Linux, celle que donne le système (sysinfo). Avertissement « juste »
//! sous 1,25 fois le besoin, « insuffisant » en dessous.
//!
//! Usage : auto-montage memoire <rush> [<vidéo suivante>…]   bilan de la préparation (texte)
//! La préparation l'appelle au début ; l'interface locale s'en sert avant de préparer et au début
//! de chaque étape.

use crate::moments_lib::{display_size, work_size};
use crate::paths;
use crate::procs;
use crate::progress::duration_of;
use crate::util::{self, read_json, round_int};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const MO: f64 = 1_048_576.0;
pub const MARGIN: f64 = 1.25; // « juste » : moins de 25 % au-dessus du besoin
pub const KEEP_MEASURES: usize = 10; // mesures gardées par opération
pub const RENDER_SHARE: f64 = 0.8; // part de la mémoire libre que le rendu peut prendre
pub const RENDER_PART: f64 = 120.0; // long montage rendu par parties de 2 min environ (render_hyperframes)

pub fn measures_path() -> PathBuf {
    paths::work().join("memoire_mesures.json")
}

/// Vidéo d'une tâche : durée du rush, tailles d'image (d'origine et de travail), durée du montage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VideoInfo {
    pub duration: f64,
    pub source: (i64, i64),
    pub work: (i64, i64),
    pub montage: f64,
    pub parts: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workers: Option<usize>,
}

fn mpx(size: (i64, i64)) -> f64 {
    size.0 as f64 * size.1 as f64 / 1e6
}

/// Navigateurs du rendu HyperFrames au plus : un par cœur, 8 au plus. La mémoire décide ensuite
/// (choose_workers).
pub fn render_workers(_hd: bool) -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 8)
}

/// Durée de montage rendue d'un coup : le montage, ou une partie (au plus 1,5 fois RENDER_PART).
fn part(v: &VideoInfo) -> f64 {
    if v.montage <= 1.5 * RENDER_PART {
        v.montage
    } else {
        1.5 * RENDER_PART
    }
}

/// Opérations connues, dans l'ordre de la version Python.
pub const OPERATIONS: [&str; 14] = [
    "Assemblage des vidéos",
    "Assemblage des vidéos (réencodage)",
    "Copie du rush en 1080p",
    "Extraction du son",
    "Détection de la parole",
    "Transcription",
    "Position du visage",
    "Bruitages et montage",
    "Consignes de montage (Claude)",
    "Montage selon les consignes",
    "Rendu de la vidéo",
    "Découpage des moments",
    "Source 4K",
    "Rendu 4K",
];

pub fn is_operation(op: &str) -> bool {
    OPERATIONS.contains(&op)
}

/// Besoin de chaque opération, en Mo : fonction de la vidéo.
fn formula(op: &str, v: &VideoInfo) -> Option<f64> {
    let workers = |hd: bool| v.workers.unwrap_or_else(|| render_workers(hd)) as f64;
    Some(match op {
        "Assemblage des vidéos" => 120.0,
        "Assemblage des vidéos (réencodage)" => 150.0 + 140.0 * mpx(v.source),
        // Décodage de l'image d'origine et encodage x264 (images d'avance) de la version de travail.
        "Copie du rush en 1080p" => 150.0 + 124.0 * mpx(v.work) + 14.0 * mpx(v.source),
        "Extraction du son" => 60.0,
        "Détection de la parole" => 60.0,
        // Modèle large-v3-turbo (1,6 Go) et ses calculs ; le son 16 kHz du rush entier en mémoire.
        "Transcription" => 2300.0 + 0.1 * v.duration,
        "Position du visage" => 100.0 + 10.0 * mpx(v.work),
        // Son du rush entier en 48 kHz (entiers puis flottants), lu pour recaler les coupes.
        "Bruitages et montage" => 250.0 + 0.3 * v.duration,
        "Consignes de montage (Claude)" => 600.0,
        "Montage selon les consignes" => 250.0 + 0.3 * v.duration,
        // Rendu : HyperFrames (node, encodeur ffmpeg, extractions d'images des plans 4 à la fois), un
        // navigateur par « worker » (plus lourd avec beaucoup de plans : un tous les 2 s de montage
        // environ), et ce qui grandit avec le montage rendu (2,3 Mo par seconde, tous navigateurs).
        "Rendu de la vidéo" => 1100.0 + workers(false) * (450.0 + 280.0 * mpx(v.work) + 0.85 * part(v)) + 2.3 * part(v),
        "Découpage des moments" => 350.0,
        "Source 4K" => 400.0 + 120.0 * 4.0 * mpx(v.work) + 14.0 * mpx(v.source),
        "Rendu 4K" => 1100.0 + workers(true) * (400.0 + 1320.0 * mpx(v.work) + 0.85 * part(v)) + 4.0 * 2.3 * part(v),
        _ => return None,
    })
}

/// Opérations de chaque étape des tâches de l'interface locale (noms de local_server).
pub fn steps(step: &str) -> Option<Vec<&'static str>> {
    Some(match step {
        "préparation du rush" => vec![
            "Copie du rush en 1080p",
            "Extraction du son",
            "Détection de la parole",
            "Transcription",
            "Position du visage",
            "Bruitages et montage",
        ],
        "consignes de montage (Claude)" => vec!["Consignes de montage (Claude)"],
        "montage selon les consignes" | "montage" => vec!["Montage selon les consignes"],
        "rendu de la vidéo entière" | "rendu 1080p" => vec!["Rendu de la vidéo"],
        "page des moments" | "mise à jour de la page" => vec!["Découpage des moments"],
        "source 4K" => vec!["Source 4K"],
        "rendu 4K" => vec!["Rendu 4K"],
        _ => return None,
    })
}

/// Opérations du rendu, et si c'est le rendu 4K.
pub fn render_hd(op: &str) -> Option<bool> {
    match op {
        "Rendu de la vidéo" => Some(false),
        "Rendu 4K" => Some(true),
        _ => None,
    }
}

/// Durée totale et tailles d'image d'un rush (une ou plusieurs vidéos, la première donne le format).
pub fn video_info(paths: &[PathBuf], montage: Option<f64>) -> VideoInfo {
    let mut duration = 0.0;
    for p in paths {
        duration += duration_of(&procs::ffmpeg_info(p));
    }
    let source = paths.first().and_then(|p| display_size(p));
    let work = source.map(|(w, h)| work_size(w, h, 1080)).unwrap_or((1080, 1920));
    VideoInfo {
        duration,
        source: source.unwrap_or(work),
        work,
        montage: montage.unwrap_or(0.6 * duration),
        parts: paths.len(),
        workers: None,
    }
}

/// Besoin d'après la formule seule, en octets.
pub fn model(op: &str, info: &VideoInfo) -> i64 {
    (formula(op, info).unwrap_or(0.0) * MO) as i64
}

pub fn load_measures() -> serde_json::Map<String, Value> {
    match read_json(measures_path()) {
        Some(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    }
}

/// (facteur appris, nombre de mesures) : plus grand rapport mesuré / prévu des 5 dernières
/// mesures, avec 10 % de marge ; (1, 0) sans mesure.
pub fn factor(op: &str, measures: &serde_json::Map<String, Value>) -> (f64, usize) {
    let all: Vec<&Value> = measures
        .get(op)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|o| util::truthy(o.get("model")) && util::truthy(o.get("peak")))
                .collect()
        })
        .unwrap_or_default();
    let obs = &all[all.len().saturating_sub(5)..];
    if obs.is_empty() {
        return (1.0, 0);
    }
    let best = obs
        .iter()
        .map(|o| util::fd(o, "peak", 0.0) / util::fd(o, "model", 1.0))
        .fold(f64::MIN, f64::max);
    ((1.1 * best).clamp(0.5, 3.0), obs.len())
}

/// Besoin prévu en octets : formule × facteur appris sur cette machine.
pub fn need(op: &str, info: &VideoInfo, measures: &serde_json::Map<String, Value>) -> i64 {
    (model(op, info) as f64 * factor(op, measures).0) as i64
}

/// Garde le pic mesuré d'une opération terminée, avec le besoin prévu par la formule.
pub fn record(op: &str, peak: i64, info: &VideoInfo) {
    let mut data = load_measures();
    let mut obs: Vec<Value> = data.get(op).and_then(Value::as_array).cloned().unwrap_or_default();
    obs.push(json!({"peak": peak, "model": model(op, info), "duration": round_int(info.duration),
                    "work": [info.work.0, info.work.1],
                    "at": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()}));
    let start = obs.len().saturating_sub(KEEP_MEASURES);
    data.insert(op.to_string(), Value::Array(obs[start..].to_vec()));
    util::mkdirs(paths::work());
    let _ = util::write_json_atomic(measures_path(), &Value::Object(data), Some(1));
}

/// Mémoire utilisée par un programme et tout ce qu'il a lancé (somme des PSS, en octets) : une
/// étape de l'interface locale. Par filiation, pas par session : les navigateurs du rendu ont la
/// leur. Hors Linux : mémoire résidente (sysinfo), un peu plus haute que la PSS.
pub fn tree_memory(root: u32) -> i64 {
    #[cfg(target_os = "linux")]
    {
        let mut children: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
        let Ok(rd) = fs::read_dir("/proc") else { return 0 };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(pid) = name.parse::<u32>() else { continue };
            let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
            let Some(after) = stat.rsplit_once(')') else { continue };
            if let Some(ppid) = after.1.split_whitespace().nth(1).and_then(|s| s.parse::<u32>().ok()) {
                children.entry(ppid).or_default().push(pid);
            }
        }
        let (mut total, mut todo) = (0i64, vec![root]);
        while let Some(pid) = todo.pop() {
            if let Some(c) = children.get(&pid) {
                todo.extend(c);
            }
            if let Ok(text) = fs::read_to_string(format!("/proc/{pid}/smaps_rollup")) {
                if let Some(line) = text.lines().find(|l| l.starts_with("Pss:")) {
                    total += line.split_whitespace().nth(1).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0) * 1024;
                }
            }
        }
        total
    }
    #[cfg(not(target_os = "linux"))]
    {
        use sysinfo::{Pid, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let mut total = 0i64;
        let mut todo = vec![Pid::from_u32(root)];
        let mut seen = std::collections::HashSet::new();
        while let Some(pid) = todo.pop() {
            if !seen.insert(pid) {
                continue;
            }
            if let Some(p) = sys.process(pid) {
                total += p.memory() as i64;
            }
            for (cpid, p) in sys.processes() {
                if p.parent() == Some(pid) {
                    todo.push(*cpid);
                }
            }
        }
        total
    }
}

/// Navigateurs du rendu : le plus possible (un par cœur, 8 au plus) tant que le besoin tient
/// dans 80 % de la mémoire libre au moment du rendu ; 1 au moins.
pub fn choose_workers(hd: bool, info: &VideoInfo, avail: Option<i64>, measures: Option<&serde_json::Map<String, Value>>) -> usize {
    let top = render_workers(hd);
    let avail = match avail.or_else(|| available().0) {
        Some(a) => a,
        None => return top.min(if hd { 2 } else { 4 }), // mémoire libre inconnue : comme avant
    };
    let owned;
    let measures = match measures {
        Some(m) => m,
        None => {
            owned = load_measures();
            &owned
        }
    };
    let op = if hd { "Rendu 4K" } else { "Rendu de la vidéo" };
    for w in (2..=top).rev() {
        let v = VideoInfo { workers: Some(w), ..info.clone() };
        if need(op, &v, measures) as f64 <= RENDER_SHARE * avail as f64 {
            return w;
        }
    }
    1
}

/// [(étape, opération, besoin en octets)] des étapes données, dans l'ordre. Rendu : avec le
/// nombre de navigateurs qu'il prendra d'après la mémoire libre (choose_workers).
pub fn operations(step_names: &[String], info: &VideoInfo, avail: Option<i64>) -> Vec<(String, String, i64)> {
    let measures = load_measures();
    let mut out = vec![];
    for step in step_names {
        let mut ops = steps(step).unwrap_or_default();
        if step == "préparation du rush" && info.parts > 1 {
            ops.insert(0, "Assemblage des vidéos (réencodage)");
        }
        for op in ops {
            let mut v = info.clone();
            if let Some(hd) = render_hd(op) {
                if info.workers.is_none() {
                    v.workers = Some(choose_workers(hd, info, avail, Some(&measures)));
                }
            }
            out.push((step.clone(), op.to_string(), need(op, &v, &measures)));
        }
    }
    out
}

/// (mémoire disponible, mémoire totale) de /proc/meminfo, en octets.
pub fn meminfo() -> (Option<i64>, Option<i64>) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = fs::read_to_string("/proc/meminfo") {
            let get = |k: &str| {
                text.lines()
                    .find(|l| l.split(':').next() == Some(k))
                    .and_then(|l| l.split(':').nth(1))
                    .and_then(|v| v.split_whitespace().next())
                    .and_then(|v| v.parse::<i64>().ok())
                    .map(|v| v * 1024)
            };
            return (get("MemAvailable"), get("MemTotal"));
        }
        (None, None)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        let total = sys.total_memory() as i64;
        if total == 0 {
            return (None, None);
        }
        (Some(sys.available_memory() as i64), Some(total))
    }
}

/// (mémoire disponible, mémoire totale) en octets : celle de la machine (ou de la machine
/// virtuelle de Docker), bornée par la limite du conteneur. (None, None) si illisible.
pub fn available() -> (Option<i64>, Option<i64>) {
    let (Some(avail), Some(total)) = meminfo() else { return (None, None) };
    // Limite du conteneur : cgroup v2, puis v1 (le cache de fichiers inactif se libère).
    for (limit_f, usage_f, stat_f, key) in [
        ("/sys/fs/cgroup/memory.max", "/sys/fs/cgroup/memory.current", "/sys/fs/cgroup/memory.stat", "inactive_file"),
        (
            "/sys/fs/cgroup/memory/memory.limit_in_bytes",
            "/sys/fs/cgroup/memory/memory.usage_in_bytes",
            "/sys/fs/cgroup/memory/memory.stat",
            "total_inactive_file",
        ),
    ] {
        let Ok(limit) = fs::read_to_string(limit_f) else { continue };
        let Ok(limit) = limit.trim().parse::<i64>() else { continue };
        if limit >= total {
            continue;
        }
        let Some(usage) = fs::read_to_string(usage_f).ok().and_then(|u| u.trim().parse::<i64>().ok()) else { continue };
        let Ok(stat) = fs::read_to_string(stat_f) else { continue };
        let inactive = stat
            .lines()
            .filter_map(|l| {
                let p: Vec<&str> = l.split_whitespace().collect();
                (p.len() == 2 && p[0] == key).then(|| p[1].parse::<i64>().unwrap_or(0))
            })
            .next()
            .unwrap_or(0);
        let free = limit - usage + inactive;
        return (Some(free.min(avail).max(0)), Some(limit));
    }
    (Some(avail), Some(total))
}

/// Place libre sur le disque qui contient `path`, en octets.
pub fn disk_free(path: &Path) -> Option<u64> {
    fs4::available_space(path).ok()
}

/// « 1,5 Go »
pub fn go(n: i64) -> String {
    util::comma(format!("{:.1} Go", n as f64 / (1u64 << 30) as f64))
}

/// « ok », « juste » (moins de 25 % de marge) ou « insuffisant ».
pub fn verdict(need: i64, avail: Option<i64>) -> &'static str {
    match avail {
        None => "ok",
        Some(a) if a < need => "insuffisant",
        Some(a) if (a as f64) < need as f64 * MARGIN => "juste",
        _ => "ok",
    }
}

/// Phrase d'avertissement pour l'utilisateur (vide si la mémoire suffit).
pub fn advice(level: &str, need: i64, avail: Option<i64>, what: &str) -> String {
    if level == "ok" {
        return String::new();
    }
    let avail = avail.unwrap_or(0);
    let head = if level == "insuffisant" {
        format!("Mémoire insuffisante pour {what} : il faut environ {}, il n'y a que {} de libres.", go(need), go(avail))
    } else {
        format!("Mémoire juste pour {what} : il faut environ {}, il y a {} de libres.", go(need), go(avail))
    };
    head + " L'étape risque d'être arrêtée en route. Fermez d'autres programmes, ou donnez plus de \
            mémoire à Docker (Réglages > Resources > Memory), puis relancez ou reprenez la tâche."
}

/// Bilan pour une tâche : besoin de chaque opération, pic, mémoire libre et avertissement.
pub fn summary(step_names: &[String], info: &VideoInfo) -> Value {
    let (avail, total) = available();
    let ops = operations(step_names, info, avail);
    // max() de Python : la première des plus grandes.
    let mut peak: (Option<String>, i64) = (None, 0);
    for (_, o, n) in &ops {
        if peak.0.is_none() || *n > peak.1 {
            peak = (Some(o.clone()), *n);
        }
    }
    let level = verdict(peak.1, avail);
    let measures = load_measures();
    json!({
        "operations": ops.iter().map(|(s, o, n)| json!({"step": s, "op": o, "need": n, "level": verdict(*n, avail),
                                                         "measured": factor(o, &measures).1})).collect::<Vec<_>>(),
        "peak": peak.1, "peakOp": peak.0, "available": avail, "total": total, "level": level,
        "message": match &peak.0 { Some(op) => advice(level, peak.1, avail, &format!("l'étape « {op} »")), None => String::new() },
    })
}

pub fn main(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage : auto-montage memoire <rush> [<vidéo suivante>…]");
        return 2;
    }
    let paths: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    let s = summary(&["préparation du rush".to_string()], &video_info(&paths, None));
    let avail = s["available"].as_i64();
    println!(
        "Mémoire : environ {} au plus fort de la préparation ({}), {}",
        go(s["peak"].as_i64().unwrap_or(0)),
        s["peakOp"].as_str().unwrap_or(""),
        match avail {
            Some(a) => format!("{} libres sur {}.", go(a), go(s["total"].as_i64().unwrap_or(0))),
            None => "mémoire libre inconnue.".into(),
        }
    );
    for o in s["operations"].as_array().unwrap() {
        let level = o["level"].as_str().unwrap_or("ok");
        println!(
            "   {} : {}{}",
            o["op"].as_str().unwrap_or(""),
            go(o["need"].as_i64().unwrap_or(0)),
            if level == "ok" { String::new() } else { format!(" ({level})") }
        );
    }
    if let Some(m) = s["message"].as_str().filter(|m| !m.is_empty()) {
        println!("ATTENTION : {m}");
    }
    0
}
