//! Couper un moment de parole en deux, ou recoller une partie (page des moments, interface locale :
//! « Couper », « Recoller »).
//!
//! Usage : auto-montage decoupage split <id> <t>    coupe à l'instant t (s depuis le début de
//!                                                  l'extrait du moment ; avant le mot le plus proche)
//!         auto-montage decoupage unsplit <id>      recolle une partie au moment précédent
//! Écrit work/decoupage.json (moments_lib : la transcription ne change pas, les ids restent
//! stables) et reporte les réglages de la page (work/local_db : moments gardés de chaque variante,
//! effets, corrections) sur les nouveaux moments. Ensuite : make_moments (extraits et page).

use crate::moments_lib::{effective_segments, load_segments, part_id, save_decoupage, Decoupage};
use crate::paths;
use crate::pyjson;
use crate::util::{self, round_to, truthy};
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;

/// Refus d'une coupe (message pour l'utilisateur).
#[derive(Debug, Clone)]
pub struct Refus(pub String);

impl std::fmt::Display for Refus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn refus<T>(msg: &str) -> Result<T, Refus> {
    Err(Refus(msg.into()))
}

fn db() -> PathBuf {
    paths::work().join("local_db")
}

fn fnum(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

fn inum(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64))).unwrap_or(0)
}

/// `float(x)` d'une valeur JSON (nombre, booléen ou texte), 0 sinon.
fn pyfloat(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::Bool(b) => *b as i64 as f64,
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Début de l'extrait d'un moment sur la page (les temps des effets en partent).
fn base(m: &Value) -> f64 {
    (fnum(m, "start") - 0.05).max(0.0)
}

fn doc_path(mid: i64) -> PathBuf {
    db().join("moments").join(format!("{mid}.json"))
}

fn dump(v: &Value) -> String {
    pyjson::dumps_with(v, pyjson::Opts { indent: Some(1), ensure_ascii: false, ..Default::default() })
}

fn read_doc(mid: i64) -> Map<String, Value> {
    let Some(d) = util::read_json(doc_path(mid)) else { return Map::new() };
    match d {
        Value::Object(o) => match o.get("data") {
            Some(Value::Object(data)) => data.clone(),
            Some(_) => Map::new(),
            None => o,
        },
        _ => Map::new(),
    }
}

/// Date de maintenant comme `datetime.now(timezone.utc).isoformat()`.
fn now_iso() -> String {
    let now = chrono::Utc::now();
    if now.timestamp_subsec_micros() == 0 {
        now.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    } else {
        now.format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()
    }
}

fn write_doc(mid: i64, d: &Map<String, Value>) {
    let path = doc_path(mid);
    if let Some(dir) = path.parent() {
        util::mkdirs(dir);
    }
    let mut d = d.clone();
    d.insert("updatedAt".into(), json!(now_iso()));
    let _ = fs::write(&path, dump(&Value::Object(d)));
}

/// Effets (sons ou visuels) décalés de dt secondes ; seulement ceux dont le départ convient.
fn shifted(items: Option<&Value>, dt: f64, keep: &dyn Fn(f64) -> bool) -> Vec<Value> {
    let mut out = vec![];
    for f in items.and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]) {
        match f {
            Value::Object(o) if o.contains_key("at") => {
                let at = if truthy(o.get("at")) { pyfloat(&o["at"]) } else { 0.0 };
                if keep(at) {
                    let mut o = o.clone();
                    o.insert("at".into(), util::num(round_to((at + dt).max(0.0), 3)));
                    out.push(Value::Object(o));
                }
            }
            _ => {
                if keep(0.0) {
                    out.push(f.clone()); // effet sans réglage (nom seul) : au début, comme avant
                }
            }
        }
    }
    out
}

fn variants() -> Vec<(PathBuf, Value)> {
    let folder = db().join("variantes");
    let mut out = vec![];
    for n in util::list_dir(&folder) {
        if n.ends_with(".json") {
            let path = folder.join(&n);
            if let Some(d) = util::read_json(&path) {
                out.push((path, d));
            }
        }
    }
    out
}

/// `x == mid` à la façon de Python (3.0 == 3).
fn same_id(x: &Value, mid: i64) -> bool {
    match x {
        Value::Number(n) => n.as_f64() == Some(mid as f64),
        Value::Bool(b) => (*b as i64) == mid,
        _ => false,
    }
}

fn kept_in(keep: &[Value], mid: i64) -> bool {
    keep.iter().any(|x| same_id(x, mid) || x.as_str().map(|s| s.split('/').next() == Some(&mid.to_string())).unwrap_or(false))
}

fn contains(keep: &[Value], mid: i64) -> bool {
    keep.iter().any(|x| same_id(x, mid))
}

/// fn(liste keep) -> nouvelle liste, pour chaque variante.
fn update_keep(f: &dyn Fn(Vec<Value>) -> Vec<Value>) {
    for (path, mut d) in variants() {
        let wrapped = d.get("data").is_some();
        let data = if wrapped { d.get_mut("data").unwrap() } else { &mut d };
        let Some(keep) = data.get("keep").and_then(Value::as_array).cloned() else { continue };
        if let Some(o) = data.as_object_mut() {
            o.insert("keep".into(), Value::Array(f(keep)));
        }
        let _ = fs::write(&path, dump(&d));
    }
}

struct State {
    raw: Vec<Value>,
    eff: Vec<Value>,
    deco: Decoupage,
    pos: HashMap<i64, usize>,
}

fn state() -> Result<State, Refus> {
    let (raw, eff, deco) = load_segments(&paths::work()).map_err(Refus)?;
    let pos = eff.iter().enumerate().map(|(i, m)| (inum(m, "id"), i)).collect();
    Ok(State { raw, eff, deco, pos })
}

/// `a or b` de Python sur deux valeurs d'un document (b, ou None, si a est faux).
fn or_value(a: Option<&Value>, b: Option<&Value>) -> Value {
    if truthy(a) {
        a.cloned().unwrap()
    } else {
        b.cloned().unwrap_or(Value::Null)
    }
}

fn list_or_empty(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

/// Début (« in ») ou fin (« out ») gardés d'un moment (poignées de la page), s'il y en a.
fn trim_at(d: &Map<String, Value>, k: &str) -> Option<f64> {
    d.get("trim").and_then(|t| t.get(k)).filter(|v| !v.is_null()).map(pyfloat)
}

fn trim_value(a: Option<f64>, b: Option<f64>) -> Value {
    if a.is_none() && b.is_none() {
        return Value::Null;
    }
    json!({"in": a.map(|x| util::num(round_to(x.max(0.0), 2))), "out": b.map(|x| util::num(round_to(x.max(0.0), 2)))})
}

pub fn split(mid: i64, t: f64) -> Result<String, Refus> {
    let State { raw, eff, mut deco, pos } = state()?;
    let Some(&pi) = pos.get(&mid) else { return refus("Moment introuvable.") };
    let m = &eff[pi];
    let words = util::arr(m, "words");
    if words.len() < 2 {
        return refus("Un seul mot dans ce moment : rien à couper.");
    }
    let at = base(m) + t;
    // Mot le plus proche de l'instant (le premier en cas d'égalité, comme min()), pas le premier :
    // chaque partie garde au moins un mot.
    let mut w = 1;
    for k in 2..words.len() {
        if (fnum(&words[k], "a") - at).abs() < (fnum(&words[w], "a") - at).abs() {
            w = k;
        }
    }
    let (o, oi) = (inum(&words[w], "o"), inum(&words[w], "oi"));
    let mut set: BTreeSet<i64> = deco.splits.get(&o.to_string()).cloned().unwrap_or_default().into_iter().collect();
    set.insert(oi);
    deco.splits.insert(o.to_string(), set.into_iter().collect());
    let second = part_id(o, oi);
    save_decoupage(&paths::work(), &deco);
    let new: HashMap<i64, Value> = effective_segments(&raw, &deco).into_iter().map(|x| (inum(&x, "id"), x)).collect();
    let t_cut = fnum(&words[w], "a") - base(m);
    let dt = new.get(&second).map(base).unwrap_or(0.0) - base(m);
    let mut d = read_doc(mid);
    if !d.is_empty() {
        let mut moved = Map::new();
        for key in ["sfx", "vfx"] {
            if d.get(key).map(Value::is_array).unwrap_or(false) {
                moved.insert(key.into(), Value::Array(shifted(d.get(key), -dt, &|x| x >= t_cut)));
            }
        }
        for key in moved.keys() {
            let v = shifted(d.get(key), 0.0, &|x| x < t_cut);
            d.insert(key.clone(), Value::Array(v));
        }
        // Début gardé : celui de la 1re partie ; fin gardée : celle de la 2e (décalée).
        let (t_in, t_out) = (trim_at(&d, "in"), trim_at(&d, "out"));
        let had_trim = d.get("trim").map(|v| !v.is_null()).unwrap_or(false);
        if had_trim {
            d.insert("trim".into(), trim_value(t_in.filter(|x| *x < t_cut), None));
        }
        d.insert("text".into(), Value::Null);
        write_doc(mid, &d);
        let focus = truthy(d.get("focus"));
        let out2 = t_out.map(|x| x - dt).filter(|x| *x > 0.0);
        if !moved.is_empty() || focus || out2.is_some() {
            // cadrage : gardé sur les deux parties
            let mut nd = Map::new();
            nd.insert("rush".into(), d.get("rush").cloned().unwrap_or(Value::Null));
            for (k, v) in moved {
                nd.insert(k, v);
            }
            if focus {
                nd.insert("focus".into(), d["focus"].clone());
            }
            if out2.is_some() {
                nd.insert("trim".into(), trim_value(None, out2));
            }
            write_doc(second, &nd);
        }
    }
    update_keep(&|mut keep: Vec<Value>| {
        if kept_in(&keep, mid) && !contains(&keep, second) {
            keep.push(json!(second));
        }
        keep
    });
    Ok(format!("Moment coupé avant « {} ».", words[w].get("w").and_then(Value::as_str).unwrap_or("")))
}

pub fn unsplit(mid: i64) -> Result<String, Refus> {
    let State { eff, mut deco, pos, .. } = state()?;
    let m = pos.get(&mid).map(|&i| &eff[i]);
    let Some(m) = m.filter(|m| m.get("splitFrom").map(|v| !v.is_null()).unwrap_or(false)) else {
        return refus("Ce moment n'est pas une partie coupée : rien à recoller.");
    };
    let i = pos[&mid];
    let o = inum(m, "splitFrom");
    let k = util::arr(m, "words").first().map(|w| inum(w, "oi")).unwrap_or(0);
    let prev = if i > 0 { Some(&eff[i - 1]) } else { None };
    let kept: Vec<i64> = deco.splits.get(&o.to_string()).cloned().unwrap_or_default().into_iter().filter(|x| *x != k).collect();
    deco.splits.insert(o.to_string(), kept);
    save_decoupage(&paths::work(), &deco);
    if let Some(prev) = prev.filter(|p| util::arr(p, "words").iter().any(|w| inum(w, "o") == o)) {
        let pid = inum(prev, "id");
        let (mut dp, dm) = (read_doc(pid), read_doc(mid));
        let dt = base(m) - base(prev);
        for key in ["sfx", "vfx"] {
            if dm.get(key).map(Value::is_array).unwrap_or(false) {
                let mut list = list_or_empty(dp.get(key));
                list.extend(shifted(dm.get(key), dt, &|_| true));
                dp.insert(key.into(), Value::Array(list));
            }
        }
        if truthy(dp.get("rush")) || truthy(dm.get("rush")) {
            let r = or_value(dp.get("rush"), dm.get("rush"));
            dp.insert("rush".into(), r);
            dp.insert("text".into(), Value::Null);
            // début gardé du moment d'avant, fin gardée de la partie recollée
            dp.insert("trim".into(), trim_value(trim_at(&dp, "in"), trim_at(&dm, "out").map(|x| x + dt)));
            write_doc(pid, &dp);
        }
        update_keep(&|keep: Vec<Value>| {
            let add = kept_in(&keep, mid) && !kept_in(&keep, pid);
            let mut out: Vec<Value> = keep.into_iter().filter(|x| !kept_in(std::slice::from_ref(x), mid)).collect();
            if add {
                out.push(json!(pid));
            }
            out
        });
    }
    Ok("Partie recollée au moment précédent.".into())
}

pub fn main(args: &[String]) -> i32 {
    let usage = || {
        eprintln!("Usage : auto-montage decoupage unsplit <id>  ou  decoupage split <id> <t>");
        2
    };
    let (Some(action), Some(id)) = (args.first(), args.get(1)) else { return usage() };
    let Ok(mid) = id.trim().parse::<i64>() else { return usage() };
    let res = match action.as_str() {
        "unsplit" => unsplit(mid),
        "split" => match args.get(2).and_then(|t| t.trim().parse::<f64>().ok()) {
            Some(t) => split(mid, t),
            None => return usage(),
        },
        _ => return usage(),
    };
    match res {
        Ok(msg) => {
            println!("{msg}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
