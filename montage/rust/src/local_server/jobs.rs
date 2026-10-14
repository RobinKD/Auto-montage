//! Tâches longues : génération de la version de travail, préparation d'un rush, création 4K,
//! analyse du style, mise à jour. Une seule à la fois ; la page suit l'étape et la fin du journal
//! (/api/job). Pause et reprise (work/tache.json), journal (work/logs/), mémoire mesurée,
//! temps restant et avancement d'ensemble.

use super::common::*;
use super::info::{style_seconds, style_info};
use crate::{memoire, paths, procs, progress, util};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

/// Une étape : (nom, commande d'auto-montage sans le programme ; « {arg} » et « {args} »
/// remplacés au lancement).
pub type Step = (String, Vec<String>);

fn step(name: &str, cmd: &[&str]) -> Step {
    (name.to_string(), cmd.iter().map(|s| s.to_string()).collect())
}

// Analyse des vidéos d'exemple : mesures (coupes, images datées, attaques sonores), puis
// lecture par Claude (ce qui a été repéré et ce qu'Auto-montage peut en reproduire).
pub fn style_steps() -> Vec<Step> {
    vec![step("mesures des vidéos d'exemple", &["analyze_style"]), step("analyse par Claude", &["style_claude"])]
}

pub fn regen_steps() -> Vec<Step> {
    let db = db().to_string_lossy().into_owned();
    vec![
        step("lecture de la sélection", &["use_selection", &db, "{arg}"]),
        step("montage", &["build_edit"]),
        step("rendu 1080p", &["render", "apercu"]),
        step("mise à jour de la page", &["make_moments"]),
    ]
}

pub fn prepare_steps() -> Vec<Step> {
    vec![
        step("préparation du rush", &["prepare", "{args}"]), // une ou plusieurs vidéos
        step("rendu de la vidéo entière", &["render", "apercu"]),
        step("page des moments", &["make_moments"]),
    ]
}

/// Version 4K (bouton des pages) : source 4K des seules images gardées, puis rendu final.
pub fn final_steps() -> Vec<Step> {
    vec![step("source 4K", &["make_hd"]), step("rendu 4K", &["render", "final"])]
}

/// Copies pour les plateformes (export.rs) : « {args} » = préréglages, plus « --couper ».
pub fn export_steps() -> Vec<Step> {
    vec![step("export pour les plateformes", &["export", "{args}"])]
}

/// Avec les consignes de montage appliquées par Claude, entre la préparation et le rendu.
pub fn prepare_with_instructions() -> Vec<Step> {
    let p = prepare_steps();
    let mut v = vec![p[0].clone()];
    v.push(step("consignes de montage (Claude)", &["apply_instructions"]));
    v.push(step("montage selon les consignes", &["build_edit"]));
    v.extend(p[1..].iter().cloned());
    v
}

// Temps restant : chaque opération (barre) et le reste de chaque étape (hors barres) durent
// « vitesse × échelle » secondes ; l'échelle est la durée du rush, celle du montage pour le
// rendu, ou 1 (durée fixe). Les vitesses de départ viennent de mesures (rush de 5 min sur un
// poste, rush de 26 s sur 2 cœurs) ; elles sont recalées après chaque tâche réussie sur les
// durées réelles de la machine (work/timings.json), et l'opération en cours suit sa propre
// vitesse dès qu'elle a avancé.
pub const PLAN: [(&str, &[&str]); 9] = [
    ("préparation du rush", &["Copie du rush en 1080p", "Extraction du son", "Transcription", "Position du visage"]),
    ("rendu de la vidéo entière", &["Rendu de la vidéo"]),
    ("rendu 1080p", &["Rendu de la vidéo"]),
    ("page des moments", &["Découpage des moments"]),
    ("mise à jour de la page", &["Découpage des moments"]),
    ("source 4K", &["Source 4K"]),
    ("rendu 4K", &["Rendu 4K"]),
    ("mesures des vidéos d'exemple", &["Analyse des vidéos d'exemple"]),
    ("export pour les plateformes", &["Export pour les plateformes"]),
];

fn plan_of(step: &str) -> &'static [&'static str] {
    PLAN.iter().find(|(s, _)| *s == step).map(|(_, ops)| *ops).unwrap_or(&[])
}

pub fn plan_json() -> Value {
    Value::Object(PLAN.iter().map(|(s, ops)| (s.to_string(), json!(ops))).collect())
}

/// (clé, échelle, secondes par seconde d'échelle, entier dans la version Python).
pub const RATES: [(&str, &str, f64, bool); 20] = [
    ("Copie du rush en 1080p", "rush", 0.3, false),
    ("Extraction du son", "rush", 0.01, false),
    ("Transcription", "rush", 0.3, false),
    ("Position du visage", "rush", 0.1, false),
    // Plusieurs vidéos pour un rush : mises bout à bout (copie) ou réencodées (réglages différents).
    ("Assemblage des vidéos", "rush", 0.03, false),
    ("Assemblage des vidéos (réencodage)", "rush", 1.2, false),
    ("Rendu de la vidéo", "montage", 3.7, false),
    ("Découpage des moments", "rush", 0.5, false),
    ("étape:préparation du rush", "rush", 0.15, false),
    ("étape:consignes de montage (Claude)", "fixe", 120.0, true),
    ("étape:rendu de la vidéo entière", "fixe", 20.0, true),
    ("étape:rendu 1080p", "fixe", 20.0, true),
    // 4K, mesuré sur 4 cœurs (rush de 5 min, montage de 39 s) : la source 4K relit tout le rush
    // (décodage 4K, ~1,9 s par seconde de rush) ; rendu 4K HyperFrames (2 navigateurs) ~13 à 30 s
    // par seconde de montage (montage de 18 s en 4 à 9 min sur 4 cœurs).
    // Analyse du style : échelle = durée totale des vidéos d'exemple ; Claude, durée fixe.
    ("Analyse des vidéos d'exemple", "rush", 0.6, false),
    ("étape:mesures des vidéos d'exemple", "fixe", 3.0, true),
    ("étape:analyse par Claude", "fixe", 150.0, true),
    ("Source 4K", "rush", 1.9, false),
    ("Rendu 4K", "montage", 20.0, true),
    ("étape:rendu 4K", "fixe", 20.0, true),
    // Export pour les plateformes (export.rs) : échelle = durée du montage pour chaque copie,
    // H.264 1080p « fast » (~1,2 s par seconde de montage et par copie sur 4 cœurs ; plusieurs
    // copies demandées : vitesse recalée sur la machine).
    ("Export pour les plateformes", "montage", 1.2, false),
    ("étape:export pour les plateformes", "fixe", 5.0, true),
];

fn rates_table() -> impl Iterator<Item = &'static (&'static str, &'static str, f64, bool)> {
    RATES.iter()
}

fn rate_kind(key: &str) -> &'static str {
    rates_table().find(|r| r.0 == key).map(|r| r.1).unwrap_or("fixe")
}

fn timings_path() -> PathBuf {
    work().join("timings.json")
}

/// Avancement affiché en temps de vidéo traité : unité de chaque opération (« rush » : secondes du
/// rush, « montage » : secondes du montage) ; les autres sont des nombres (segments…).
fn unit_of(label: &str) -> Value {
    if label == "Segments transcrits seuls" {
        return json!("segments");
    }
    match rates_table().find(|r| r.0 == label && r.1 != "fixe") {
        Some(r) => json!(r.1),
        None => Value::Null,
    }
}

// Pause et reprise (préparation d'un rush) : la tâche en cours est décrite dans work/tache.json
// (étapes, étape atteinte, avancement), mis à jour à chaque étape. « Mettre en pause » arrête les
// programmes en gardant ce qui est fait (morceaux de la version de travail, morceaux transcrits,
// positions du visage, extraits : work/reprise/, work/seg/, work/moments/clips/) ; « Reprendre »
// relance l'étape atteinte avec AM_REPRISE=1, qui repart de là. Une tâche coupée par un arrêt du
// serveur ou de l'ordinateur (fichier resté « running »), ou arrêtée par une erreur,
// se reprend de la même façon.
pub fn task_path() -> PathBuf {
    work().join("tache.json")
}
pub const RESUMABLE: [&str; 1] = ["prepare"];

fn resumable(kind: &str) -> bool {
    RESUMABLE.contains(&kind)
}

/// État de la tâche : le dictionnaire `job` de la version Python (clés JSON dans l'ordre), plus
/// ce qui n'est pas du JSON (programme lancé, journal ouvert, vidéo de la mesure de mémoire).
pub struct Job {
    pub m: Map<String, Value>,
    pub pid: Option<u32>,
    pub log_file: Option<fs::File>,
    pub mem_info: Option<memoire::VideoInfo>,
}

pub static JOB: LazyLock<Mutex<Job>> = LazyLock::new(|| {
    let m = match json!({"state": "idle", "kind": null, "step": "", "error": null, "log": [], "progress": null, "spent": {}}) {
        Value::Object(m) => m,
        _ => unreachable!(),
    };
    Mutex::new(Job { m, pid: None, log_file: None, mem_info: None })
});

/// Le `job_lock` de la version Python : vérifier puis lancer (ou supprimer) sans qu'une autre
/// demande passe entre les deux.
pub static JOB_LOCK: Mutex<()> = Mutex::new(());

pub fn lock() -> std::sync::MutexGuard<'static, Job> {
    JOB.lock().unwrap_or_else(|e| e.into_inner())
}

fn gs<'a>(m: &'a Map<String, Value>, k: &str) -> &'a str {
    m.get(k).and_then(Value::as_str).unwrap_or("")
}
fn gf(m: &Map<String, Value>, k: &str) -> f64 {
    m.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}
fn truthy(m: &Map<String, Value>, k: &str) -> bool {
    util::truthy(m.get(k))
}
fn obj<'a>(m: &'a mut Map<String, Value>, k: &str) -> &'a mut Map<String, Value> {
    if !matches!(m.get(k), Some(Value::Object(_))) {
        m.insert(k.into(), json!({}));
    }
    m.get_mut(k).unwrap().as_object_mut().unwrap()
}
fn fnum(x: f64) -> Value {
    util::num(x)
}

pub fn now() -> f64 {
    util::now_secs()
}

/// Durée du montage : celui de cette tâche s'il est déjà fait, sinon une estimation.
pub fn montage_seconds(m: &Map<String, Value>) -> f64 {
    let path = edit_json();
    // Seule la préparation refait le montage plus tard : sinon edit.json est le bon.
    let fresh = gs(m, "kind") != "prepare" || util::mtime(&path).map(|t| t >= gf(m, "startedTs")).unwrap_or(false);
    if fresh {
        if let Some(edit) = util::read_json(&path) {
            if let (Some(d), Some(fps)) = (edit.get("durationInFrames").and_then(Value::as_f64), edit.get("fps").and_then(Value::as_f64)) {
                if fps != 0.0 {
                    return d / fps;
                }
            }
        }
    }
    0.6 * gf(m, "rushSeconds") // montage générique : à peu près la parole du rush
}

/// Vitesses : celles de départ, recalées par work/timings.json.
pub fn load_rates() -> Map<String, Value> {
    let mut rates = Map::new();
    for (k, _, v, int) in rates_table() {
        rates.insert(k.to_string(), if *int { json!(*v as i64) } else { fnum(*v) });
    }
    if let Some(Value::Object(file)) = util::read_json(timings_path()) {
        let mut parsed = Map::new();
        for (k, v) in file {
            match py_float(&v) {
                Some(x) => {
                    parsed.insert(k, fnum(x));
                }
                None => return rates_defaults_only(),
            }
        }
        for (k, v) in parsed {
            rates.insert(k, v);
        }
    }
    rates
}

fn rates_defaults_only() -> Map<String, Value> {
    rates_table().map(|(k, _, v, int)| (k.to_string(), if *int { json!(*v as i64) } else { fnum(*v) })).collect()
}

pub fn rate(rates: &Map<String, Value>, key: &str) -> f64 {
    rates.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn estimate(key: &str, rates: &Map<String, Value>, m: &Map<String, Value>) -> f64 {
    let scale = match rate_kind(key) {
        "rush" => gf(m, "rushSeconds"),
        "montage" => montage_seconds(m),
        _ => 1.0,
    };
    rate(rates, key) * scale
}

/// Recale les vitesses sur les durées mesurées de la tâche qui vient de réussir.
fn learn_timings(m: &Map<String, Value>) {
    let mut rates = load_rates();
    for (key, kind, _, _) in rates_table() {
        let Some(spent) = m.get("spent").and_then(|s| s.get(*key)).and_then(Value::as_f64) else { continue };
        let scale = match *kind {
            "rush" => gf(m, "rushSeconds"),
            "montage" => montage_seconds(m),
            _ => 1.0,
        };
        if scale <= 0.0 {
            continue;
        }
        let old = rates.get(*key).and_then(Value::as_f64).unwrap_or(spent / scale);
        rates.insert(key.to_string(), fnum(util::round_to(0.3 * old + 0.7 * spent / scale, 4)));
    }
    let _ = write_json(timings_path(), &Value::Object(rates), Some(1), true);
}

/// Durée de l'opération en cours, comptée à son libellé (et retirée du reste de l'étape).
fn end_phase(m: &mut Map<String, Value>, now: f64) {
    let Some(Value::Object(p)) = m.get_mut("progress") else { return };
    if util::truthy(p.get("ended")) {
        return;
    }
    p.insert("ended".into(), json!(true));
    let label = p.get("label").and_then(Value::as_str).unwrap_or("").to_string();
    let start = p.get("start").cloned().unwrap_or(json!(0.0));
    let since = p.get("since").and_then(Value::as_f64).unwrap_or(now);
    let op_start = obj(m, "opStart");
    if !op_start.contains_key(&label) {
        op_start.insert(label.clone(), start);
    }
    let spent = obj(m, "spent");
    let before = spent.get(&label).and_then(Value::as_f64).unwrap_or(0.0);
    spent.insert(label, fnum(before + now - since));
    let phases = gf(m, "stepPhases");
    m.insert("stepPhases".into(), fnum(phases + now - since));
}

fn set_progress(m: &mut Map<String, Value>, label: &str, done: f64, total: f64) {
    let now = now();
    let current = m.get("progress").cloned().filter(|p| p.is_object());
    if let Some(c) = &current {
        if c["label"] != label {
            end_phase(m, now);
        }
    }
    let same = current.as_ref().map(|c| c["label"] == label).unwrap_or(false);
    let since = if same { current.as_ref().unwrap()["since"].clone() } else { fnum(now) };
    // Part déjà faite quand l'opération commence (reprise après une pause) : la vitesse mesurée
    // ne compte que ce qui avance pendant cette tâche.
    let start = if same {
        current.as_ref().unwrap().get("start").cloned().unwrap_or(json!(0.0))
    } else if total != 0.0 {
        fnum((done / total).min(1.0))
    } else {
        json!(0.0)
    };
    m.insert("progress".into(), json!({"label": label, "done": fnum(done), "total": fnum(total), "since": since, "at": fnum(now), "start": start}));
    let step = m.get("step").cloned().unwrap_or(Value::Null);
    obj(m, "ops").insert(label.to_string(), json!({"done": fnum(done), "total": fnum(total), "unit": unit_of(label), "step": step}));
    m.insert("memOp".into(), Value::Null); // mémoire comptée pour cette opération (sample_memory)
}

fn progress_obj(m: &Map<String, Value>) -> Option<&Map<String, Value>> {
    m.get("progress").and_then(Value::as_object)
}

fn running_progress(m: &Map<String, Value>) -> Option<&Map<String, Value>> {
    progress_obj(m).filter(|p| !util::truthy(p.get("ended")))
}

/// Rapport durée réelle / prévue des opérations déjà finies de cette tâche (machine plus
/// lente ou plus rapide que prévu) : il corrige aussi les opérations à venir.
fn speed_factor(m: &Map<String, Value>, rates: &Map<String, Value>) -> f64 {
    let (mut real, mut planned) = (0.0, 0.0);
    if let Some(Value::Object(spent)) = m.get("spent") {
        for (key, s) in spent {
            let kind = rates_table().find(|r| r.0 == key).map(|r| r.1);
            if matches!(kind, Some(k) if k != "fixe") {
                real += s.as_f64().unwrap_or(0.0);
                let f0 = m.get("opStart").and_then(|o| o.get(key)).and_then(Value::as_f64).unwrap_or(0.0);
                planned += estimate(key, rates, m) * (1.0 - f0);
            }
        }
    }
    if planned < 1.0 || real < 1.0 {
        return 1.0;
    }
    (real / planned).clamp(0.1, 10.0)
}

fn steps_of(m: &Map<String, Value>) -> Vec<String> {
    m.get("steps")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// (reste de l'opération en cours, reste de la tâche), en secondes.
fn remaining_time(m: &Map<String, Value>) -> (Option<f64>, Option<f64>) {
    let base = load_rates();
    let k = speed_factor(m, &base);
    let rates: Map<String, Value> = base
        .iter()
        .map(|(key, v)| {
            let x = v.as_f64().unwrap_or(0.0) * if rate_kind(key) != "fixe" { k } else { 1.0 };
            (key.clone(), fnum(x))
        })
        .collect();
    let now = now();
    let steps = steps_of(m);
    let step = gs(m, "step").to_string();
    let Some(i) = steps.iter().position(|s| *s == step) else { return (None, None) };
    let plan = plan_of(&step);
    let p = progress_obj(m);
    let mut current = None;
    let mut upcoming: &[&str] = plan;
    let running = p.filter(|p| !util::truthy(p.get("ended")));
    if let Some(p) = running {
        let (done, total) = (p.get("done").and_then(Value::as_f64).unwrap_or(0.0), p.get("total").and_then(Value::as_f64).unwrap_or(0.0));
        let f = if total != 0.0 { (done / total).min(1.0) } else { 0.0 };
        let f0 = p.get("start").and_then(Value::as_f64).unwrap_or(0.0);
        let label = p.get("label").and_then(Value::as_str).unwrap_or("");
        let model = estimate(label, &rates, m) * (1.0 - f);
        let elapsed = now - p.get("since").and_then(Value::as_f64).unwrap_or(now);
        let live = if f > f0 && elapsed > 3.0 { elapsed * (1.0 - f) / (f - f0) } else { model };
        let w = ((f - f0) / 0.25).min(1.0); // vitesse mesurée de plus en plus suivie
        current = Some(w * live + (1.0 - w) * model);
        if let Some(idx) = plan.iter().position(|o| *o == label) {
            upcoming = &plan[idx + 1..];
        }
    } else if let Some(p) = p {
        let label = p.get("label").and_then(Value::as_str).unwrap_or("");
        if let Some(idx) = plan.iter().position(|o| *o == label) {
            upcoming = &plan[idx + 1..];
        }
    }
    let mut rest = current.unwrap_or(0.0) + upcoming.iter().map(|k| estimate(k, &rates, m)).sum::<f64>();
    // Reste de l'étape hors barres (installation de Whisper, découpage de l'audio, montage…).
    let in_op = running.map(|p| now - p.get("since").and_then(Value::as_f64).unwrap_or(now)).unwrap_or(0.0);
    let outside = now - gf(m, "stepStart") - gf(m, "stepPhases") - in_op;
    rest += (estimate(&format!("étape:{step}"), &rates, m) - outside).max(0.0);
    for s in &steps[i + 1..] {
        rest += plan_of(s).iter().map(|k| estimate(k, &rates, m)).sum::<f64>() + estimate(&format!("étape:{s}"), &rates, m);
    }
    (current, Some(rest))
}

/// Part de chaque étape et de ses opérations dans la barre d'ensemble, fixée au début de la
/// tâche (durées prévues sans la vitesse mesurée ensuite) : la barre ne recule pas quand le
/// temps restant s'allonge.
fn step_weights(m: &Map<String, Value>) -> Value {
    let rates = load_rates();
    let mut out = Map::new();
    for s in steps_of(m) {
        let ops: Map<String, Value> = plan_of(&s).iter().map(|k| (k.to_string(), fnum(estimate(k, &rates, m)))).collect();
        out.insert(s.clone(), json!({"ops": ops, "rest": fnum(estimate(&format!("étape:{s}"), &rates, m))}));
    }
    Value::Object(out)
}

/// Avancement de la tâche (0 à 1) d'après la vidéo traitée : étapes finies, puis dans l'étape
/// en cours, temps de rush (ou de montage) traité par chacune de ses opérations. Seul le reste de
/// l'étape hors barres (Claude, installation…) suit le temps passé, sans dépasser 95 %. Jamais
/// en recul.
fn overall_progress(m: &mut Map<String, Value>) -> f64 {
    let weights = m.get("weights").and_then(Value::as_object).cloned().unwrap_or_default();
    let steps = steps_of(m);
    let step = gs(m, "step").to_string();
    let Some(i) = steps.iter().position(|s| *s == step) else { return gf(m, "overallMax") };
    let wsum = |w: &Value| -> f64 {
        w["ops"].as_object().map(|o| o.values().filter_map(Value::as_f64).sum::<f64>()).unwrap_or(0.0) + w["rest"].as_f64().unwrap_or(0.0)
    };
    let total: f64 = weights.values().map(wsum).sum();
    let mut done: f64 = steps[..i].iter().filter_map(|s| weights.get(s)).map(wsum).sum();
    let w = weights.get(&step).cloned().unwrap_or_else(|| json!({"ops": {}, "rest": 0.0}));
    let p = running_progress(m).cloned();
    if let Some(ops) = w["ops"].as_object() {
        for (k, est) in ops {
            let Some(o) = m.get("ops").and_then(|o| o.get(k)).filter(|o| util::truthy(Some(o))) else { continue };
            if o.get("step").and_then(Value::as_str) != Some(step.as_str()) {
                continue;
            }
            let running = p.as_ref().map(|p| p.get("label").and_then(Value::as_str) == Some(k.as_str())).unwrap_or(false);
            let (od, ot) = (o.get("done").and_then(Value::as_f64).unwrap_or(0.0), o.get("total").and_then(Value::as_f64).unwrap_or(0.0));
            let f = if ot != 0.0 { (od / ot).min(1.0) } else { 0.0 };
            let spent = m.get("spent").and_then(|s| s.get(k)).is_some();
            done += est.as_f64().unwrap_or(0.0) * if running || !spent { f } else { 1.0 };
        }
    }
    let rest = w["rest"].as_f64().unwrap_or(0.0);
    if rest != 0.0 && truthy(m, "stepStart") {
        let now = now();
        let in_op = p.as_ref().map(|p| now - p.get("since").and_then(Value::as_f64).unwrap_or(now)).unwrap_or(0.0);
        let outside = now - gf(m, "stepStart") - gf(m, "stepPhases") - in_op;
        done += rest * (outside.max(0.0) / rest).min(0.95);
    }
    let f = if total > 0.0 { done / total } else { i as f64 / steps.len().max(1) as f64 };
    let best = gf(m, "overallMax").max(f.min(1.0));
    m.insert("overallMax".into(), fnum(best));
    best
}

/// Message d'erreur à montrer : la ligne « Error… » ou « …Error: » plutôt que la pile Node.
fn job_error(log: &[Value], code: i32) -> String {
    let lines: Vec<&str> = log.iter().filter_map(Value::as_str).collect();
    for line in lines.iter().rev() {
        let s = line.trim();
        if s.starts_with("Error") || s.contains("Error:") || s.starts_with("ERREUR") || s.starts_with("Erreur") {
            return s.to_string();
        }
    }
    for line in lines.iter().rev() {
        if !line.trim().starts_with("at ") {
            return line.trim().to_string();
        }
    }
    progress::exit_reason(code, true)
}

static PROGRESS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@progression ([\d.e+-]+) ([\d.e+-]+) (.+)$").unwrap());

/// (libellé, fait, total) d'une ligne d'avancement (progress). None sinon.
pub fn progress_of(line: &str) -> Option<(String, f64, f64)> {
    let c = PROGRESS.captures(line)?;
    Some((c[3].to_string(), c[1].parse().ok()?, c[2].parse().ok()?))
}

/// Écrit au journal de la tâche (work/logs/…). Journal impossible à écrire (disque plein…) : dit
/// une fois sur la page et dans le journal du serveur (work/logs/demarrage.log), qui suit le reste.
fn write(j: &mut Job, text: &str) {
    let Some(f) = j.log_file.as_mut() else { return };
    if let Err(e) = f.write_all(format!("{text}\n").as_bytes()) {
        if !truthy(&j.m, "logError") {
            let msg = format!("journal de la tâche non écrit ({e}) : disque plein ?");
            eprintln!("Auto-montage : {msg} ; {}", progress::resources(&logs()));
            j.m.insert("logError".into(), json!(msg));
        }
        eprintln!("{text}");
    }
}

enum End {
    Paused,
    Cancelled,
    Failed(String),
}

fn hms() -> String {
    now_fmt("%H:%M:%S")
}

/// Commande d'une étape : « {args} » remplacé par les vidéos, « {arg} » par l'objet de la tâche.
fn argv(cmd: &[String], arg: &str, args: &[String]) -> Vec<String> {
    let mut out = vec![];
    for c in cmd {
        if c == "{args}" {
            out.extend(args.iter().cloned());
        } else {
            out.push(c.replace("{arg}", arg));
        }
    }
    out
}

/// Sortie d'un programme lue ligne par ligne, « \r » compris (comme le mode texte de Python).
fn for_each_line(reader: impl Read, mut f: impl FnMut(&str)) {
    let mut r = BufReader::new(reader);
    let mut buf = vec![];
    loop {
        buf.clear();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let mut chunk = &buf[..];
                if chunk.ends_with(b"\n") {
                    chunk = &chunk[..chunk.len() - 1];
                    if chunk.ends_with(b"\r") {
                        chunk = &chunk[..chunk.len() - 1];
                    }
                }
                let text = String::from_utf8_lossy(chunk);
                for part in text.split('\r') {
                    f(part);
                }
            }
        }
    }
}

fn run_job(steps: Vec<Step>, arg: String, args: Vec<String>, first: usize) {
    let result = run_steps(&steps, &arg, &args, first);
    let mut j = lock();
    match result {
        Ok(()) => {}
        Err(End::Paused) => {
            drop(j);
            remove_partial_files(false);
            j = lock();
            j.m.insert("state".into(), json!("paused"));
            j.m.insert("error".into(), Value::Null);
            j.pid = None;
            j.m.insert("finishedAt".into(), json!(now_iso()));
            save_task(&j, "paused");
            j.m.insert("progress".into(), Value::Null);
            let step = gs(&j.m, "step").to_string();
            write(&mut j, &format!("\n# RÉSULTAT : pause (étape « {step} » ; « Reprendre » la continue)"));
        }
        Err(End::Cancelled) => {
            drop(j);
            remove_partial_files(false);
            j = lock();
            if resumable(gs(&j.m, "kind")) {
                remove_task();
            }
            j.m.insert("state".into(), json!("cancelled"));
            j.m.insert("error".into(), Value::Null);
            j.m.insert("progress".into(), Value::Null);
            j.pid = None;
            j.m.insert("finishedAt".into(), json!(now_iso()));
            let step = gs(&j.m, "step").to_string();
            write(&mut j, &format!("\n# RÉSULTAT : annulé (étape « {step} »)"));
        }
        Err(End::Failed(mut e)) => {
            if let Some(le) = j.m.get("logError").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                e = format!("{e} ({le})");
            }
            j.m.insert("state".into(), json!("error"));
            j.m.insert("error".into(), json!(e));
            j.pid = None;
            j.m.insert("finishedAt".into(), json!(now_iso()));
            if resumable(gs(&j.m, "kind")) {
                // reprise possible une fois le problème réglé (mémoire, place…)
                save_task(&j, "error");
            }
            eprintln!("Erreur de la tâche : {e}");
            let step = gs(&j.m, "step").to_string();
            write(&mut j, &format!("\nErreur : {e}\n# RÉSULTAT : échec (étape « {step} ») : {e}"));
        }
    }
    j.log_file = None;
}

fn run_steps(steps: &[Step], arg: &str, args: &[String], first: usize) -> Result<(), End> {
    let (kind, resumed, reuse) = {
        let j = lock();
        (gs(&j.m, "kind").to_string(), truthy(&j.m, "resumed"), truthy(&j.m, "reuse"))
    };
    let rush = if kind == "prepare" {
        rushes().join(arg)
    } else {
        root().join(read_text(work().join("rush.txt")).unwrap_or_default())
    };
    // Plusieurs vidéos à assembler : le rush assemblé n'existe pas encore (ou va être refait).
    let rush_seconds = if args.len() > 1 {
        args.iter().map(|a| media_seconds(&rushes().join(a))).sum()
    } else if kind == "style" {
        style_seconds()
    } else {
        media_seconds(&rush)
    };
    let montage = {
        let mut j = lock();
        j.m.insert("rushSeconds".into(), fnum(rush_seconds));
        let w = step_weights(&j.m);
        j.m.insert("weights".into(), w);
        montage_seconds(&j.m)
    };
    // Mémoire : besoin de la tâche (son étape la plus gourmande) et de chaque étape.
    let vpaths: Vec<PathBuf> = if kind == "prepare" { args.iter().map(|a| rushes().join(a)).collect() } else { vec![rush.clone()] };
    let names: Vec<String> = steps[first..].iter().map(|(n, _)| n.clone()).collect();
    if names.iter().any(|n| memoire::steps(n).is_some()) {
        let info = memoire::video_info(&vpaths, if kind == "prepare" { None } else { Some(montage) });
        let plan = memoire::summary(&names, &info);
        let mut j = lock();
        j.mem_info = Some(info);
        let mut mem = Map::new();
        for k in ["peak", "peakOp", "available", "total", "level", "message"] {
            mem.insert(k.into(), plan[k].clone());
        }
        j.m.insert("memory".into(), Value::Object(mem));
        let message = plan["message"].as_str().unwrap_or("");
        let text = format!(
            "# Mémoire : environ {} au plus fort ({}), {} libres sur {}{}",
            memoire::go(plan["peak"].as_i64().unwrap_or(0)),
            plan["peakOp"].as_str().unwrap_or("None"),
            memoire::go(plan["available"].as_i64().unwrap_or(0)),
            memoire::go(plan["total"].as_i64().unwrap_or(0)),
            if message.is_empty() { String::new() } else { format!("\n# ATTENTION : {message}") }
        );
        write(&mut j, &text);
    } else {
        lock().mem_info = None; // tâche sans étape lourde (mise à jour, analyse du style…)
    }
    {
        let mut j = lock();
        if resumed {
            write(&mut j, &format!("# Reprise à l'étape « {} »", steps[first].0));
        }
        if reuse {
            write(&mut j, &format!("# Parties gardées d'une préparation abandonnée : {}", kept_parts(args).join(", ")));
        }
    }
    // Reprise, ou parties gardées par un abandon : ce qui est déjà fait n'est pas refait.
    let reprise = resumed || reuse;
    for (index, (step, cmd)) in steps.iter().enumerate() {
        if index < first {
            continue; // reprise : étapes déjà faites
        }
        let argv = argv(cmd, arg, args);
        {
            let mut j = lock();
            let now = now();
            if truthy(&j.m, "stepStart") {
                end_phase(&mut j.m, now);
                let key = format!("étape:{}", gs(&j.m, "step"));
                let spent = obj(&mut j.m, "spent").get(&key).and_then(Value::as_f64).unwrap_or(0.0);
                let add = now - gf(&j.m, "stepStart") - gf(&j.m, "stepPhases");
                obj(&mut j.m, "spent").insert(key, fnum(spent + add));
            }
            j.m.insert("step".into(), json!(step));
            j.m.insert("stepIndex".into(), json!(index));
            j.m.insert("progress".into(), Value::Null);
            j.m.insert("stepStart".into(), fnum(now));
            j.m.insert("stepPhases".into(), json!(0.0));
            save_task(&j, "running");
            write(&mut j, &format!("\n## {} Étape : {step} (auto-montage {})", hms(), cmd.join(" ")));
            check_memory(&mut j, step);
            if truthy(&j.m, "cancel") {
                return Err(if truthy(&j.m, "pause") { End::Paused } else { End::Cancelled });
            }
        }
        // Groupe de processus à part : « Annuler » arrête le programme et tout ce qu'il a lancé.
        let (reader, writer) = std::io::pipe().map_err(|e| End::Failed(e.to_string()))?;
        let mut command = Command::new(paths::self_exe());
        command.args(&argv).current_dir(root()).stdin(Stdio::null());
        command.stdout(writer.try_clone().map_err(|e| End::Failed(e.to_string()))?).stderr(writer);
        if reprise {
            command.env("AM_REPRISE", "1");
        }
        procs::new_group(&mut command);
        let spawned = command.spawn();
        drop(command); // ferme nos copies de l'écriture : la lecture finit avec le programme
        let mut child = spawned.map_err(|e| End::Failed(format!("{e}")))?;
        let pid = child.id();
        {
            let mut j = lock();
            j.pid = Some(pid);
            j.m.insert("memOp".into(), Value::Null);
            j.m.insert("memPeaks".into(), json!({}));
            j.m.insert("renderWorkers".into(), Value::Null);
        }
        let finished = Arc::new(AtomicBool::new(false));
        let sampler = {
            let finished = finished.clone();
            let step = step.clone();
            std::thread::spawn(move || sample_memory(pid, &step, &finished))
        };
        let mut last_progress = 0.0;
        static NAV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Navigateurs du rendu : (\d+)").unwrap());
        for_each_line(reader, |raw| {
            // journal en direct (progression du rendu, étapes…)
            let line = raw.trim_end();
            let mut j = lock();
            if let Some(op) = line.strip_prefix("@memoire ") {
                // opération en cours, sans barre d'avancement
                j.m.insert("memOp".into(), json!(op.trim()));
                return;
            }
            if let Some(c) = NAV.captures(line) {
                // nombre choisi d'après la mémoire libre : compte dans la mesure du rendu
                if let Ok(n) = c[1].parse::<i64>() {
                    j.m.insert("renderWorkers".into(), json!(n));
                }
            }
            let mut found = if line.is_empty() { None } else { progress_of(line) };
            if let Some(f) = found.as_mut() {
                if gs(&j.m, "kind") == "final" && f.0 == "Rendu de la vidéo" {
                    f.0 = "Rendu 4K".into();
                }
            }
            if let Some((label, done, total)) = found {
                // affiché en barre, pas au journal (un point d'avancement toutes les 30 s)
                set_progress(&mut j.m, &label, done, total);
                if now() - last_progress > 30.0 {
                    last_progress = now();
                    let text = format!(
                        "   … {label} : {} / {} ({})",
                        util::fmt_g(done, 6),
                        util::fmt_g(total, 6),
                        progress::resources(root())
                    );
                    write(&mut j, &text);
                    watch_memory(&mut j);
                    save_task(&j, "running"); // avancement gardé si tout s'arrête d'un coup
                }
                return;
            }
            if !line.is_empty() {
                let log = j.m.entry("log").or_insert_with(|| json!([]));
                if let Value::Array(a) = log {
                    a.push(json!(line));
                    if a.len() > 200 {
                        let cut = a.len() - 200;
                        a.drain(..cut);
                    }
                }
                write(&mut j, line);
            }
        });
        let code = procs::wait(&mut child);
        finished.store(true, Ordering::SeqCst);
        lock().pid = None;
        let _ = sampler.join();
        let mut j = lock();
        if truthy(&j.m, "cancel") {
            return Err(if truthy(&j.m, "pause") { End::Paused } else { End::Cancelled });
        }
        write(&mut j, &format!("## {} Fin de l'étape ({})", hms(), progress::exit_reason(code, true)));
        if code != 0 {
            write(&mut j, &format!("   {}", progress::resources(root())));
            let log = j.m.get("log").and_then(Value::as_array).cloned().unwrap_or_default();
            return Err(End::Failed(job_error(&log, code)));
        }
        record_memory(&mut j, reprise && index == first);
    }
    let mut j = lock();
    let now = now();
    end_phase(&mut j.m, now);
    let key = format!("étape:{}", gs(&j.m, "step"));
    let spent = obj(&mut j.m, "spent").get(&key).and_then(Value::as_f64).unwrap_or(0.0);
    let add = now - gf(&j.m, "stepStart") - gf(&j.m, "stepPhases");
    obj(&mut j.m, "spent").insert(key, fnum(spent + add));
    if !reprise {
        learn_timings(&j.m); // durées d'une tâche reprise : pas représentatives
    }
    if resumable(&kind) {
        remove_task(); // une mise à jour garde la préparation en pause
    }
    j.m.insert("state".into(), json!("done"));
    j.m.insert("step".into(), json!("terminé"));
    j.m.insert("progress".into(), Value::Null);
    j.m.insert("finishedAt".into(), json!(now_iso()));
    let secs = util::round_int(util::now_secs() - gf(&j.m, "startedTs"));
    write(&mut j, &format!("\n# RÉSULTAT : réussi ({secs} s)"));
    if kind == "update" || kind == "reset" {
        // programme remplacé : relance de la nouvelle version
        j.m.insert("step".into(), json!("redémarrage"));
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(3));
            crate::relance::relaunch();
        });
    }
    Ok(())
}

// --- Pause et reprise (work/tache.json) -----------------------------------------------
/// Décrit la tâche en cours (reprenable) dans work/tache.json : de quoi la reprendre.
fn save_task(j: &Job, state: &str) {
    let m = &j.m;
    if !resumable(gs(m, "kind")) {
        return;
    }
    let elapsed = gf(m, "before") + now() - gf(m, "startedTs");
    let current = progress_obj(m).map(|p| p.get("label").cloned().unwrap_or(Value::Null)).unwrap_or(Value::Null);
    let started = if truthy(m, "firstStartedAt") { m["firstStartedAt"].clone() } else { m.get("startedAt").cloned().unwrap_or(Value::Null) };
    let g = |k: &str| m.get(k).cloned().unwrap_or(Value::Null);
    let rec = json!({"state": state, "kind": g("kind"), "arg": g("arg"), "args": g("args"), "steps": g("steps"),
                     "index": m.get("stepIndex").cloned().unwrap_or(json!(0)), "step": g("step"), "ops": g("ops"),
                     "elapsed": util::round_int(elapsed), "langue": g("langue"), "startedAt": started,
                     "error": g("error"), "at": now_iso_secs(), "current": current});
    util::mkdirs(work());
    let part = work().join("tache.json.part");
    let res = write_json(&part, &rec, Some(1), false).and_then(|_| fs::rename(&part, task_path()));
    if let Err(e) = res {
        eprintln!("tache.json : {e}");
    }
}

pub fn remove_path(path: &Path) {
    util::remove_all(path);
}

/// Efface la préparation en pause : seulement à la fin (réussie ou annulée) d'une préparation,
/// jamais à celle d'une autre tâche (mise à jour, réinitialisation : permises pendant la pause).
fn remove_task() {
    remove_path(&task_path());
    remove_path(&work().join("reprise"));
}

// « Abandonner » une préparation : chaque partie déjà faite peut être effacée ou gardée. Une
// partie gardée resservira à la prochaine préparation du même rush (work/reprise/garde.json :
// préparation lancée comme une reprise, AM_REPRISE=1, sans rien refaire de ce qui est gardé).
fn reprise() -> PathBuf {
    work().join("reprise")
}
fn kept_path() -> PathBuf {
    reprise().join("garde.json")
}

pub fn abandon_parts_def() -> Vec<(&'static str, &'static str, Vec<PathBuf>)> {
    vec![
        ("travail", "Copie du rush en 1080p", vec![rushes().join("rush_1080.mp4"), reprise().join("travail")]),
        ("son", "Son et transcription", ["rush_16k.wav", "vad.txt", "seg", "segments.json"].iter().map(|n| work().join(n)).collect()),
        ("visage", "Position du visage", vec![reprise().join("visage.json")]),
        ("moments", "Extraits de la page des moments", ["clips", "thumbs"].iter().map(|n| moments().join(n)).collect()),
    ]
}

/// Vidéos d'un rush (nom, taille, date) : une partie gardée ne resert qu'au même rush.
fn rush_fingerprint(args: &[String]) -> Value {
    Value::Array(
        args.iter()
            .map(|a| match fs::metadata(rushes().join(a)) {
                Ok(st) => json!([a, st.len(), util::mtime(rushes().join(a)).unwrap_or(0.0) as i64]),
                Err(_) => json!([a, null, null]),
            })
            .collect(),
    )
}

/// Parties déjà faites de la préparation en pause : [{id, label, size}] (celles qui existent).
pub fn abandon_parts() -> Vec<Value> {
    let mut out = vec![];
    for (pid, label, paths) in abandon_parts_def() {
        let size: u64 = paths
            .iter()
            .filter(|p| p.exists())
            .map(|p| if p.is_dir() { dir_size(p) } else { fs::metadata(p).map(|m| m.len()).unwrap_or(0) })
            .sum();
        if paths.iter().any(|p| p.exists()) {
            out.push(json!({"id": pid, "label": label, "size": size}));
        }
    }
    out
}

/// Abandonne la préparation en pause : efface les parties de « delete » (toutes sans liste),
/// garde les autres pour la prochaine préparation du même rush.
pub fn abandon_task(delete: Option<Vec<String>>) -> Vec<String> {
    let rec = paused_task().unwrap_or_default();
    let defs = abandon_parts_def();
    let ids: Vec<String> = defs.iter().map(|d| d.0.to_string()).collect();
    let delete: Vec<String> = match delete {
        None => ids.clone(),
        Some(d) => d.into_iter().filter(|x| ids.contains(x)).collect(),
    };
    let mut kept: Vec<String> = vec![];
    for (pid, _, paths) in &defs {
        if delete.iter().any(|d| d == pid) {
            for p in paths {
                remove_path(p);
            }
        } else if paths.iter().any(|p| p.exists()) {
            kept.push(pid.to_string());
        }
    }
    remove_path(&task_path());
    // Restes de reprise des parties effacées (la version de travail en cours : son.flac…).
    let has = |k: &str| kept.iter().any(|x| x == k);
    for name in util::list_dir(reprise()) {
        if !((name == "travail" && has("travail")) || (name == "visage.json" && has("visage"))) {
            remove_path(&reprise().join(&name));
        }
    }
    let args: Vec<String> = rec.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default();
    if !kept.is_empty() && util::truthy(rec.get("args")) {
        util::mkdirs(reprise());
        let _ = write_json(kept_path(), &json!({"args": rec["args"], "rush": rush_fingerprint(&args), "kept": kept}), None, false);
    }
    kept
}

/// Parties gardées par un abandon pour ces vidéos (même rush), ou [].
pub fn kept_parts(args: &[String]) -> Vec<String> {
    if let Some(Value::Object(rec)) = util::read_json(kept_path()) {
        if rec.get("args") == Some(&json!(args)) && rec.get("rush") == Some(&rush_fingerprint(args)) {
            return rec
                .get("kept")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                .unwrap_or_default();
        }
    }
    vec![]
}

/// Tâche à reprendre (en pause, coupée par un arrêt, ou arrêtée par une erreur), ou None.
/// Pendant qu'une tâche tourne, celle-ci n'en est pas une.
pub fn paused_task_state(state: &str) -> Option<Map<String, Value>> {
    if state == "running" {
        return None;
    }
    let Some(Value::Object(mut rec)) = util::read_json(task_path()) else { return None };
    if !resumable(rec.get("kind").and_then(Value::as_str).unwrap_or("")) || !rec.get("steps").map(Value::is_array).unwrap_or(false) {
        return None;
    }
    if rec.get("state").and_then(Value::as_str) == Some("running") {
        rec.insert("state".into(), json!("interrompu")); // le serveur s'est arrêté pendant la tâche
    }
    Some(rec)
}

pub fn paused_task() -> Option<Map<String, Value>> {
    let state = gs(&lock().m, "state").to_string();
    paused_task_state(&state)
}

/// Message d'une tâche refusée : une autre tourne, ou une préparation attend d'être reprise.
pub fn busy_error() -> String {
    if let Some(rec) = paused_task() {
        let arg = match rec.get("arg") {
            Some(Value::String(s)) => s.clone(),
            Some(v) => crate::pyjson::dumps(v),
            None => String::new(),
        };
        return format!("La préparation de {arg} est en pause : reprenez-la ou abandonnez-la d'abord (page « Rushes et montages »).");
    }
    "Une tâche est en cours : attendez qu'elle finisse.".into()
}

pub fn is_running() -> bool {
    lock().m["state"] == "running"
}

pub fn busy() -> bool {
    is_running() || paused_task().is_some()
}

/// Reprend la tâche en pause à l'étape où elle s'était arrêtée. Message d'erreur, ou None.
pub fn resume_job() -> Option<String> {
    let Some(rec) = paused_task() else { return Some("Aucune tâche à reprendre.".into()) };
    let every = prepare_with_instructions();
    let names: Vec<String> = rec["steps"].as_array().unwrap().iter().map(|s| s.as_str().unwrap_or("").to_string()).collect();
    if !names.iter().all(|n| every.iter().any(|(e, _)| e == n)) {
        return Some("Cette tâche ne peut pas être reprise : abandonnez-la.".into());
    }
    let args: Vec<String> = rec.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default();
    if let Some(missing) = args.iter().find(|a| !rushes().join(a).is_file()) {
        return Some(format!("La vidéo {missing} n'est plus dans le dossier des rushes : la préparation ne peut pas reprendre."));
    }
    let index = rec.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    if names.iter().skip(index).any(|n| n == "consignes de montage (Claude)") && !super::claude::claude_connected(false) {
        return Some("Claude Code n'est pas connecté : connectez-le (page « Connexion à Claude »), puis reprenez.".into());
    }
    if let Some(l) = rec.get("langue").and_then(Value::as_str) {
        if crate::moments_lib::language_label(l).is_some() {
            // langue de la transcription, lue par prepare
            let _ = fs::write(work().join("langue.txt"), format!("{l}\n"));
        }
    }
    let steps: Vec<Step> = names.iter().map(|n| every.iter().find(|(e, _)| e == n).unwrap().clone()).collect();
    let kind = rec.get("kind").and_then(Value::as_str).unwrap_or("prepare").to_string();
    let arg = rec.get("arg").and_then(Value::as_str).unwrap_or("").to_string();
    if !start_job(&kind, steps, &arg, Some(args), Some(rec), false) {
        return Some(busy_error());
    }
    None
}

// --- Mémoire (memoire) --------------------------------------------------------
/// Au début de chaque étape : son besoin de mémoire comparé à la mémoire libre maintenant.
fn check_memory(j: &mut Job, step: &str) {
    if j.mem_info.is_none() {
        return;
    }
    if ["rendu de la vidéo entière", "rendu 1080p", "rendu 4K", "source 4K"].contains(&step) {
        let m = montage_seconds(&j.m); // montage fait entre-temps
        j.mem_info.as_mut().unwrap().montage = m;
    }
    let ops = memoire::operations(&[step.to_string()], j.mem_info.as_ref().unwrap(), None);
    let Some(first) = ops.first() else { return };
    let mut best = first.clone();
    for o in &ops {
        if o.2 > best.2 {
            best = o.clone();
        }
    }
    let (_, op, need) = best;
    let (avail, _total) = memoire::available();
    let level = memoire::verdict(need, avail);
    let message = memoire::advice(level, need, avail, &format!("l'étape « {op} »"));
    j.m.insert("stepMemory".into(), json!({"step": step, "op": op, "need": need, "available": avail, "level": level, "message": message}));
    let text = format!(
        "   Mémoire de l'étape : environ {} ({op}), {} libres{}",
        memoire::go(need),
        memoire::go(avail.unwrap_or(0)),
        if level == "ok" { String::new() } else { format!(" : {}", level.to_uppercase()) }
    );
    write(j, &text);
}

/// Pendant une étape : pic de mémoire de chaque opération (somme des PSS de l'étape et de tout
/// ce qu'elle a lancé, une fois par seconde), dans job["memPeaks"]. L'opération est celle de la barre
/// d'avancement en cours, ou d'une ligne « @memoire <opération> », ou la seule de l'étape.
fn sample_memory(pid: u32, step: &str, finished: &AtomicBool) {
    let alone = memoire::steps(step).unwrap_or_default();
    while !finished.load(Ordering::SeqCst) {
        let used = memoire::tree_memory(pid);
        {
            let mut j = lock();
            let mem_op = j.m.get("memOp").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
            let label = progress_obj(&j.m).and_then(|p| p.get("label")).and_then(Value::as_str).filter(|l| memoire::is_operation(l)).map(String::from);
            let op = mem_op.or(label).or_else(|| (alone.len() == 1).then(|| alone[0].to_string()));
            if let Some(op) = op.filter(|o| memoire::is_operation(o)) {
                if used > 0 {
                    let peaks = obj(&mut j.m, "memPeaks");
                    let before = peaks.get(&op).and_then(Value::as_i64).unwrap_or(0);
                    peaks.insert(op, json!(before.max(used)));
                }
            }
        }
        for _ in 0..10 {
            if finished.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Étape réussie : pics mesurés gardés (work/memoire_mesures.json) pour les prochaines
/// estimations. Étape reprise en route : pas gardés (une partie du travail était déjà faite).
fn record_memory(j: &mut Job, partial: bool) {
    let peaks = j.m.get("memPeaks").and_then(Value::as_object).cloned().unwrap_or_default();
    let Some(info) = j.mem_info.clone() else { return };
    if peaks.is_empty() {
        return;
    }
    let text: Vec<String> = peaks.iter().map(|(op, n)| format!("{op} {}", memoire::go(n.as_i64().unwrap_or(0)))).collect();
    write(j, &format!("   Mémoire mesurée : {}", text.join(", ")));
    if partial {
        return;
    }
    let workers = j.m.get("renderWorkers").and_then(Value::as_u64).filter(|w| *w > 0);
    for (op, peak) in peaks {
        let mut v = info.clone();
        if let (Some(w), Some(_)) = (workers, memoire::render_hd(&op)) {
            v.workers = Some(w as usize);
        }
        memoire::record(&op, peak.as_i64().unwrap_or(0), &v);
    }
}

/// Pendant une étape : mémoire presque épuisée (moins de 300 Mo ou de 4 % libres).
fn watch_memory(j: &mut Job) {
    let (Some(avail), total) = memoire::available() else { return };
    let total = total.unwrap_or(0) as f64;
    let low = (avail as f64) < (300.0 * memoire::MO).max(0.04 * total);
    let mut sm = j.m.get("stepMemory").and_then(Value::as_object).cloned().unwrap_or_default();
    if low && !util::truthy(sm.get("low")) {
        sm.insert("low".into(), json!(true));
        sm.insert("level".into(), json!("insuffisant"));
        sm.insert("available".into(), json!(avail));
        sm.insert(
            "message".into(),
            json!(format!(
                "La mémoire est presque épuisée ({} libres) : l'étape risque d'être arrêtée. Fermez d'autres programmes (navigateur, logiciels de montage…), ou mettez la préparation en pause le temps d'en fermer.",
                memoire::go(avail)
            )),
        );
        j.m.insert("stepMemory".into(), Value::Object(sm));
        write(j, &format!("   ATTENTION : mémoire presque épuisée ({} libres)", memoire::go(avail)));
    }
}

// --- Journal (page /logs/) ------------------------------------------------------------
// Chaque tâche écrit son journal complet dans work/logs/<date>_<tâche>.log (les 40 derniers
// sont gardés) ; le démarrage (installation de ce qui manque) et le serveur écrivent dans
// work/logs/demarrage.log (`auto-montage start`, lanceur.rs). La page « Journal » les affiche
// et en fait un rapport à envoyer.
pub fn kind_name(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "prepare" => "Préparation",
        "regen" => "Génération",
        "final" => "Création 4K",
        "update" => "Mise à jour",
        "reset" => "Réinitialisation",
        "style" => "Analyse du style",
        "export" => "Export pour les plateformes",
        _ => return None,
    })
}

fn digit_prefix(name: &str) -> bool {
    name.len() >= 4 && name.as_bytes()[..4].iter().all(u8::is_ascii_digit)
}

fn open_job_log(kind: &str, arg: &str, steps: &[String]) -> std::io::Result<(fs::File, String)> {
    util::mkdirs(logs());
    let name = format!("{}_{kind}.log", now_fmt("%Y-%m-%d_%H%M%S"));
    let mut f = fs::File::create(logs().join(&name))?;
    write!(
        f,
        "# {} : {arg}\n# Auto-montage {}, démarré le {}\n# Étapes : {}\n\n",
        kind_name(kind).unwrap_or(kind),
        *VERSION,
        now_fmt("%d/%m/%Y à %H:%M:%S"),
        steps.join(", ")
    )?;
    let logs_list: Vec<String> = util::list_dir(logs()).into_iter().filter(|n| digit_prefix(n) && n.ends_with(".log")).collect();
    if logs_list.len() > 40 {
        for old in &logs_list[..logs_list.len() - 40] {
            let _ = fs::remove_file(logs().join(old));
        }
    }
    Ok((f, name))
}

fn tail_bytes(path: &Path, n: u64) -> Vec<u8> {
    let Ok(mut f) = fs::File::open(path) else { return vec![] };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    use std::io::Seek;
    let _ = f.seek(std::io::SeekFrom::Start(size.saturating_sub(n)));
    let mut buf = vec![];
    let _ = f.read_to_end(&mut buf);
    buf
}

/// Au démarrage du serveur, aucune tâche ne tourne : un journal sans « # RÉSULTAT » est celui
/// d'une tâche coupée par l'arrêt du serveur ou de l'ordinateur. Il est complété pour le dire.
pub fn close_interrupted_logs() {
    let names = util::list_dir(logs());
    let start = names.len().saturating_sub(40);
    for name in &names[start..] {
        if !(digit_prefix(name) && name.ends_with(".log")) {
            continue;
        }
        let path = logs().join(name);
        let tail = String::from_utf8_lossy(&tail_bytes(&path, 600)).into_owned();
        if tail.lines().any(|l| l.starts_with("# RÉSULTAT : ")) {
            continue;
        }
        let last = local_fmt(util::mtime(&path).unwrap_or(0.0), "%d/%m/%Y à %H:%M:%S");
        if let Ok(mut f) = fs::OpenOptions::new().append(true).open(&path) {
            let _ = write!(
                f,
                "\n# Dernière écriture le {last}, puis plus rien : le serveur s'est arrêté pendant la tâche \
                 (Auto-montage fermé, ordinateur éteint ou redémarré, mémoire épuisée…).\n\
                 # Serveur redémarré le {} ({}).\n# RÉSULTAT : interrompu\n",
                now_fmt("%d/%m/%Y à %H:%M:%S"),
                progress::resources(root())
            );
        }
    }
}

pub fn log_list() -> Vec<Value> {
    static RESULT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^# RÉSULTAT : (\w+)").unwrap());
    let (state_now, log_name) = {
        let j = lock();
        (gs(&j.m, "state").to_string(), j.m.get("logName").and_then(Value::as_str).map(String::from))
    };
    let mut out = vec![];
    let mut names = util::list_dir(logs());
    names.reverse();
    for name in names {
        let path = logs().join(&name);
        if !name.ends_with(".log") || !path.is_file() {
            continue;
        }
        let tail = String::from_utf8_lossy(&tail_bytes(&path, 600)).into_owned();
        let found: Vec<String> = RESULT.captures_iter(&tail).map(|c| c[1].to_string()).collect();
        let state = match found.last() {
            Some(s) => s.clone(),
            None if state_now == "running" && log_name.as_deref() == Some(name.as_str()) => "running".into(),
            None => "interrompu".into(),
        };
        let dated = digit_prefix(&name);
        let kind = if dated { name.get(18..name.len() - 4).unwrap_or("").to_string() } else { name[..name.len() - 4].to_string() };
        let other = match kind.as_str() {
            "demarrage" => Some("Démarrage et serveur"),
            "demarrage.prec" => Some("Démarrage (journal précédent)"),
            "discussion" => Some("Discussion avec Claude (erreurs)"),
            _ => None,
        };
        let label = kind_name(&kind).or(other).map(String::from).unwrap_or_else(|| kind.clone());
        out.push(json!({"name": name, "kind": if dated { kind.clone() } else { "demarrage".into() },
                        "label": label, "state": if dated { state } else { "serveur".into() },
                        "size": fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                        "modified": iso_local_secs(util::mtime(&path).unwrap_or(0.0))}));
    }
    out
}

/// Rapport à joindre à une demande d'aide : état du système, journaux récents.
pub fn report_text() -> String {
    let mut lines = vec![format!("Rapport Auto-montage {} — {}", *VERSION, now_fmt("%d/%m/%Y %H:%M:%S")), String::new()];
    let osr = read_text("/etc/os-release").unwrap_or_default();
    let system = osr
        .lines()
        .find(|l| l.starts_with("PRETTY_NAME="))
        .map(|l| l.split_once('=').unwrap().1.trim_matches('"').to_string())
        .unwrap_or_else(|| "?".into());
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let (state, kind, step, error) = {
        let j = lock();
        let s = |k: &str| match j.m.get(k) {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        (s("state"), s("kind"), s("step"), s("error"))
    };
    lines.push(format!("Système : {system} ; {cpus} cœurs ; disque libre {} Go", disk_free(root()) >> 30));
    lines.push(format!(
        "Claude Code : {}{}",
        if super::claude::claude_connected(false) { "connecté" } else { "non connecté" },
        if paths::claude().is_some() { "" } else { " (non installé)" }
    ));
    lines.push(format!(
        "Rush en cours : {} ; page des moments : {}",
        read_text(work().join("rush.txt")).filter(|r| !r.is_empty()).unwrap_or_else(|| "aucun".into()),
        if moments().join("index.html").exists() { "oui" } else { "non" }
    ));
    lines.push(format!("Tâche : {state} {kind} {step} {error}"));
    lines.push(String::new());
    let logs_l = log_list();
    let jobs: Vec<&Value> = logs_l.iter().filter(|l| l["kind"] != "demarrage").take(3).collect();
    let bar = "=".repeat(78);
    for l in jobs {
        let name = l["name"].as_str().unwrap_or("");
        lines.push(bar.clone());
        lines.push(format!("{} ({name}) : {}", l["label"].as_str().unwrap_or(""), l["state"].as_str().unwrap_or("")));
        lines.push(bar.clone());
        lines.push(String::from_utf8_lossy(&fs::read(logs().join(name)).unwrap_or_default()).into_owned());
        lines.push(String::new());
    }
    let start = logs().join("demarrage.log");
    if start.exists() {
        let text = String::from_utf8_lossy(&fs::read(&start).unwrap_or_default()).into_owned();
        let all: Vec<&str> = text.lines().collect();
        lines.push(bar.clone());
        lines.push("Démarrage et serveur (300 dernières lignes)".into());
        lines.push(bar);
        lines.extend(all[all.len().saturating_sub(300)..].iter().map(|s| s.to_string()));
    }
    lines.join("\n") + "\n"
}

/// Fichiers en cours d'écriture (*.part.*, renommés seulement une fois complets) d'une tâche
/// annulée : les versions précédentes restent intactes. Avec uploads (démarrage du serveur, rien
/// ne tourne) : aussi les envois coupés (.envoi-*) et les fichiers d'une tâche arrêtée en route.
pub fn remove_partial_files(uploads: bool) {
    for folder in [out(), out().join("export"), rushes()] {
        for name in util::list_dir(&folder) {
            if name.contains(".part.") || (uploads && name.starts_with(".envoi-")) {
                let _ = fs::remove_file(folder.join(name));
            }
        }
    }
    if uploads {
        // images extraites par HyperFrames restées d'un rendu (place::extract_caches)
        for cache in crate::place::extract_caches() {
            let _ = fs::remove_dir_all(cache);
        }
    }
}

/// Arrête la tâche en cours ; avec pause, elle est gardée pour être reprise (préparation).
/// La pause arrête les programmes (comme l'annulation) : la reprise relance l'étape avec
/// AM_REPRISE=1, ce qui marche aussi sous Windows (arbre de processus arrêté).
pub fn cancel_job(pause: bool) -> bool {
    let _g = JOB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let pid = {
        let mut j = lock();
        if j.m["state"] != "running" || (pause && !resumable(gs(&j.m, "kind"))) {
            return false;
        }
        j.m.insert("cancel".into(), json!(true));
        j.m.insert("pause".into(), json!(pause));
        j.pid
    };
    if let Some(pid) = pid {
        procs::kill_group(pid, procs::Sig::Term);
    }
    true
}

/// arg : objet de la tâche (rush, variante…) ; args : vidéos du rush (« {args} »), [arg] par défaut.
/// resume : tâche en pause (work/tache.json) reprise à son étape. reuse : préparation qui reprend
/// les parties gardées par un abandon (work/reprise/garde.json). Refusée si une tâche tourne ou
/// si une préparation attend d'être reprise (sauf pour la reprendre).
pub fn start_job(kind: &str, steps: Vec<Step>, arg: &str, args: Option<Vec<String>>, resume: Option<Map<String, Value>>, reuse: bool) -> bool {
    let _g = JOB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let args = args.filter(|a| !a.is_empty()).unwrap_or_else(|| vec![arg.to_string()]);
    let first;
    {
        let mut j = lock();
        let state = gs(&j.m, "state").to_string();
        if state == "running" || (resume.is_none() && kind != "update" && kind != "reset" && paused_task_state(&state).is_some()) {
            return false;
        }
        let r = resume.clone().unwrap_or_default();
        first = r.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let names: Vec<String> = steps.iter().map(|(n, _)| n.clone()).collect();
        let started = now();
        let langue = match r.get("langue") {
            Some(v) if util::truthy(Some(v)) => v.clone(),
            _ => json!(crate::moments_lib::language(&work())),
        };
        let before = r.get("elapsed").filter(|v| util::truthy(Some(v))).and_then(py_float).unwrap_or(0.0);
        let ops = match r.get("ops") {
            Some(Value::Object(o)) => Value::Object(o.clone()),
            _ => json!({}),
        };
        let first_started = r.get("startedAt").cloned().unwrap_or(Value::Null);
        let m = &mut j.m;
        for (k, v) in [
            ("state", json!("running")),
            ("kind", json!(kind)),
            ("step", json!("démarrage")),
            ("error", Value::Null),
            ("logError", Value::Null),
            ("log", json!([])),
            ("arg", json!(arg)),
            ("progress", Value::Null),
            ("cancel", json!(false)),
            ("pause", json!(false)),
            ("logName", Value::Null),
            ("spent", json!({})),
            ("opStart", json!({})),
            ("stepStart", Value::Null),
            ("stepPhases", json!(0.0)),
            ("rushSeconds", json!(0.0)),
            ("steps", json!(names)),
            ("args", json!(args)),
            ("stepIndex", json!(first)),
            ("ops", ops),
            ("resumed", json!(resume.is_some())),
            ("reuse", json!(reuse)),
            ("before", fnum(before)),
            ("firstStartedAt", first_started),
            ("weights", json!({})),
            ("overallMax", json!(0.0)),
            ("memory", Value::Null),
            ("stepMemory", Value::Null),
            ("langue", langue),
            ("startedAt", json!(now_iso())),
            ("startedTs", fnum(started)),
        ] {
            m.insert(k.to_string(), v);
        }
        j.pid = None;
        j.mem_info = None;
        j.log_file = None;
        let title = format!(
            "{arg}{}{}",
            if args.len() > 1 { format!(" ({})", args.join(" + ")) } else { String::new() },
            if resume.is_some() { " : reprise" } else { "" }
        );
        match open_job_log(kind, &title, &names) {
            Ok((f, name)) => {
                j.log_file = Some(f);
                j.m.insert("logName".into(), json!(name));
            }
            Err(e) => eprintln!("Journal de la tâche : {e}"),
        }
    }
    let arg = arg.to_string();
    std::thread::spawn(move || run_job(steps, arg, args, first));
    true
}

pub fn job_status() -> Value {
    const HIDDEN: [&str; 11] = ["log", "spent", "proc", "cancel", "logFile", "opStart", "memInfo", "memPeaks", "memOp", "renderWorkers", "weights"];
    let mut j = lock();
    let mut status: Map<String, Value> = j.m.iter().filter(|(k, _)| !HIDDEN.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
    let log = j.m.get("log").and_then(Value::as_array).cloned().unwrap_or_default();
    status.insert("log".into(), json!(log[log.len().saturating_sub(30)..]));
    status.insert("now".into(), fnum(now()));
    let state = gs(&j.m, "state").to_string();
    status.insert("paused".into(), paused_task_state(&state).map(Value::Object).unwrap_or(Value::Null));
    status.insert("plan".into(), plan_json());
    if state == "running" {
        let (current, rest) = remaining_time(&j.m);
        status.insert("remaining".into(), rest.map(|r| json!(util::round_int(r))).unwrap_or(Value::Null));
        let overall = overall_progress(&mut j.m);
        status.insert("overall".into(), fnum(util::round_to(overall, 4)));
        if let (Some(Value::Object(p)), Some(c)) = (status.get_mut("progress"), current) {
            p.insert("remaining".into(), json!(util::round_int(c)));
        }
    } else {
        status.insert("overall".into(), json!(if state == "done" { 1.0 } else { 0.0 }));
    }
    Value::Object(status)
}

/// Style : les vidéos d'exemple sont-elles là (pour /api/prepare) ?
pub fn style_has_content() -> bool {
    let s = style_info();
    !util::arr(&s, "videos").is_empty() || !util::arr(&s, "sons").is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines() {
        assert_eq!(progress_of("@progression 12.5 300 Transcription"), Some(("Transcription".into(), 12.5, 300.0)));
        assert_eq!(progress_of("autre"), None);
    }

    #[test]
    fn argv_expands() {
        let cmd = vec!["prepare".to_string(), "{args}".into()];
        assert_eq!(argv(&cmd, "x", &["a.mov".into(), "b.mov".into()]), vec!["prepare", "a.mov", "b.mov"]);
        let cmd = vec!["use_selection".to_string(), "db".into(), "{arg}".into()];
        assert_eq!(argv(&cmd, "travail", &[]), vec!["use_selection", "db", "travail"]);
    }

    #[test]
    fn errors_like_python() {
        let log = vec![json!("Error: boom"), json!("    at x")];
        assert_eq!(job_error(&log, 1), "Error: boom");
        assert_eq!(job_error(&[], 137), "code 137, signal SIGKILL : tué de force, le plus souvent par manque de mémoire (fermez d'autres programmes)");
    }

    #[test]
    fn lines_split_on_cr() {
        let mut got = vec![];
        for_each_line(&b"a\r\nb\rc\nd"[..], |l| got.push(l.to_string()));
        assert_eq!(got, vec!["a", "b", "c", "d"]);
    }
}
