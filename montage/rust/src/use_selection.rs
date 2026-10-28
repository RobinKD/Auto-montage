//! Applique une variante choisie sur la page des moments.
//!
//! Usage : auto-montage use_selection <dossier d'export> <variante>
//! <dossier d'export> = ce que Claude a exporté de la page (ArtifactData avec out_dir) :
//!   variantes/<variante>.json  {"name", "rush", "keep": [ids de segments], "updatedAt"}
//!   moments/<id>.json          {"rush", "text"?: texte corrigé, "nosub"?: true (sans sous-titres),
//!                               "sfx"?: ["cash", …] ou [{"kind", "at", "repeat", "gap", "dur"}, …],
//!                               "vfx"?: [{"kind": "typed" | "chip", "text", "at", "type", "dur", "sound"}, …],
//!                               "trim"?: {"in", "out"} : début et fin gardés (s depuis le début
//!                               de l'extrait ; poignées de la page),
//!                               "focus"?: n (cadrage sur la personne n de detect_face), "auto"
//!                               (personne qui parle, diarize) ou "tous" (tout le monde),
//!                               "transition"?: {"kind", "dur"} (coupe avant ce moment, look.rs)}
//! Les documents des moments sont aussi gardés tels quels (« page » : sélection et réglages),
//! repris par build_edit dans edit.json, pour que la page montre ce qui a changé depuis.
//! Écrit work/selection.json, que build_edit utilise à la place de KEEP, avec les
//! corrections de sous-titres et les effets sonores choisis par moment.

use crate::paths;
use crate::pyjson;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;

/// `repr()` de Python pour une valeur lue dans un JSON (chaîne entre apostrophes, None…), pour les
/// messages d'erreur identiques à ceux du script d'origine.
pub(crate) fn py_repr(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".into(),
        Some(Value::Bool(b)) => if *b { "True" } else { "False" }.into(),
        Some(Value::String(s)) => {
            let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
            let mut out = String::new();
            out.push(q);
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c == q => {
                        out.push('\\');
                        out.push(c);
                    }
                    c => out.push(c),
                }
            }
            out.push(q);
            out
        }
        Some(v) => pyjson::dumps_utf8(v),
    }
}

/// Document exporté : brut ({id, data, version}) ou données seules.
fn load(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{} : {e}", path.display()))?;
    Ok(match doc.get("data") {
        Some(d) => d.clone(),
        None => doc,
    })
}

/// Nombre au sens de `isinstance(x, (int, float))` (un booléen en est un pour Python).
fn as_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn num(x: f64) -> Value {
    crate::util::num(x)
}

pub fn main(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage : auto-montage use_selection <dossier d'export> <variante>");
        return 2;
    }
    let (export_dir, variant) = (Path::new(&args[0]), &args[1]);
    let work = paths::work();
    let rush_txt = fs::read_to_string(work.join("rush.txt")).unwrap_or_default();
    let rush = Path::new(rush_txt.trim()).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

    let doc = match load(&export_dir.join("variantes").join(format!("{variant}.json"))) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    if doc.get("rush").and_then(Value::as_str) != Some(rush.as_str()) {
        eprintln!(
            "Cette sélection vise {}, le rush en cours est {}",
            py_repr(doc.get("rush")),
            py_repr(Some(&json!(rush)))
        );
        return 1;
    }
    let Some(keep) = doc.get("keep").cloned() else {
        eprintln!("Variante sans « keep » : {}", export_dir.join("variantes").join(format!("{variant}.json")).display());
        return 1;
    };

    let (mut corrections, mut sfx, mut vfx, mut trims, mut gains, mut focus, mut transitions) =
        (Map::new(), Map::new(), Map::new(), Map::new(), Map::new(), Map::new(), Map::new());
    // Réglages de la page tels qu'ils partent dans la version de travail : la page les compare
    // aux réglages du moment pour signaler « Modifié depuis la version de travail ».
    const PAGE_FIELDS: [&str; 8] = ["text", "nosub", "sfx", "vfx", "trim", "gain", "focus", "transition"];
    let mut page = Map::new();
    // Ordre du dossier, comme glob.glob (os.scandir).
    let entries: Vec<_> = fs::read_dir(export_dir.join("moments"))
        .map(|it| it.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    for path in entries {
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if !name.ends_with(".json") || name.starts_with('.') {
            continue;
        }
        let Ok(m) = load(&path) else { continue };
        if m.get("rush").and_then(Value::as_str) != Some(rush.as_str()) {
            continue;
        }
        let seg = name[..name.len() - 5].to_string();
        let mut fields = Map::new();
        for k in PAGE_FIELDS {
            if let Some(v) = m.get(k) {
                if !v.is_null() {
                    fields.insert(k.into(), v.clone());
                }
            }
        }
        page.insert(seg.clone(), Value::Object(fields));
        if m.get("nosub") == Some(&Value::Bool(true)) {
            // sous-titres supprimés sur la page : texte vide
            corrections.insert(seg.clone(), json!(""));
        } else if let Some(t) = m.get("text").and_then(Value::as_str) {
            if !t.trim().is_empty() {
                corrections.insert(seg.clone(), json!(t.trim()));
            }
        }
        if let Some(list) = m.get("sfx").and_then(Value::as_array) {
            let kept: Vec<Value> = list.iter().filter(|s| s.is_string() || s.is_object()).cloned().collect();
            sfx.insert(seg.clone(), Value::Array(kept));
        }
        if let Some(Value::Object(t)) = m.get("trim") {
            // début et fin gardés : {"in", "out"} en s
            let mut out = Map::new();
            for k in ["in", "out"] {
                if let Some(x) = t.get(k).and_then(as_number) {
                    out.insert(k.into(), num(x));
                }
            }
            trims.insert(seg.clone(), Value::Object(out));
        }
        if let Some(g) = m.get("gain").and_then(as_number) {
            // volume de la voix du moment
            if g != 1.0 {
                gains.insert(seg.clone(), num(g.clamp(0.0, 2.0)));
            }
        }
        if let Some(list) = m.get("vfx").and_then(Value::as_array) {
            vfx.insert(seg.clone(), Value::Array(list.iter().filter(|v| v.is_object()).cloned().collect()));
        }
        match m.get("focus") {
            // cadrage sur une personne (entier seulement, comme isinstance(…, int))
            Some(Value::Number(n)) => {
                if let Some(i) = n.as_i64() {
                    focus.insert(seg.clone(), json!(i));
                }
            }
            // « auto » : personne qui parle (diarize) ; « tous » : tout le monde dans l'image
            Some(Value::String(v)) if v == "auto" || v == "tous" => {
                focus.insert(seg.clone(), json!(v));
            }
            _ => {}
        }
        if let Some(t) = m.get("transition").and_then(crate::look::transition_of) {
            // transition avant ce moment (look.rs), à la place du réglage général
            transitions.insert(seg.clone(), Value::Object(t));
        }
    }

    let n_keep = keep.as_array().map(|a| a.len()).unwrap_or(0);
    let (n_corr, n_sfx, n_vfx, n_trims) = (corrections.len(), sfx.len(), vfx.len(), trims.len());
    let name = doc.get("name").cloned().unwrap_or(Value::Null);
    let out = json!({
        "name": name, "keep": keep, "corrections": corrections, "sfx": sfx, "vfx": vfx,
        "trims": trims, "gains": gains, "focus": focus,
        "transitions": transitions, "page": {"keep": keep, "moments": page},
    });
    let text = pyjson::dumps_with(&out, pyjson::Opts { indent: Some(1), ensure_ascii: false, ..Default::default() });
    if let Err(e) = fs::write(work.join("selection.json"), text) {
        eprintln!("{} : {e}", work.join("selection.json").display());
        return 1;
    }
    let shown = match &name {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        v => pyjson::dumps_utf8(v),
    };
    println!(
        "Variante « {shown} » : {n_keep} moments, {n_corr} textes corrigés, {n_sfx} moments avec effets sonores et \
         {n_vfx} avec effets visuels choisis, {n_trims} raccourcis -> work/selection.json"
    );
    0
}
