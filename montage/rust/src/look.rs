//! Retouche des couleurs, effets d'image et transitions (repris de FilmCraft) : réglages et
//! listes côté programme. Le dessin est fait par selection/hf_look.js, le même code pour le rendu
//! (render_hyperframes, page HyperFrames) et pour l'aperçu de la page des moments.
//!
//!   work/couleur.json      couleurs de tout le montage : température, teinte, exposition (IL),
//!                          contraste, hautes, ombres, blancs, noirs (-100 à 100), saturation (%),
//!                          vibrance, look (préréglage créatif) et son intensité (%), vignette (%).
//!   work/transitions.json  transition entre deux moments : {"kind": id ou "", "dur": s}.
//!   moments/<id>.json      « transition » : {"kind", "dur"} pour la coupe avant ce moment
//!                          (« kind » vide : coupe franche), à la place du réglage général.
//!   effet visuel « filtre » d'un moment : {"kind": "filtre", "effect": id, "at", "dur", "force"}.
//!
//! build_edit en tire « couleur », « looks » et « transitions » d'edit.json (pas en dérushage).

use crate::paths;
use crate::util;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

/// Transitions de selection/hf_look.js (TRANSITIONS), même ordre.
pub const TRANSITIONS: [&str; 23] = [
    "fondu", "fondunoir", "fondublanc", "fonduflou", "fonduadditif", "fondufilm", "balayage", "balayagedoux", "volets",
    "horloge", "iris", "losange", "ouverture", "poussee", "glissement", "etirement", "fouet", "zoomcroise", "flash", "seisme",
    "glitch", "retournement", "tele",
];
/// Effets d'image de selection/hf_look.js (EFFECTS).
pub const EFFECTS: [&str; 16] = [
    "noirblanc", "sepia", "negatif", "teinte", "flou", "floudir", "nettete", "lueur", "posteriser", "mosaique", "contours", "bruit",
    "vhs", "stroboscope", "miroir", "envers",
];
/// Looks de selection/hf_look.js (LOOKS).
pub const LOOKS: [&str; 8] = ["chaud", "froid", "cinema", "vintage", "delave", "vif", "nb", "nuit"];

/// Réglages de couleur : (clé, défaut, min, max).
pub const COLOR_KEYS: [(&str, f64, f64, f64); 12] = [
    ("temperature", 0.0, -100.0, 100.0),
    ("teinte", 0.0, -100.0, 100.0),
    ("exposition", 0.0, -3.0, 3.0),
    ("contraste", 0.0, -100.0, 100.0),
    ("hautes", 0.0, -100.0, 100.0),
    ("ombres", 0.0, -100.0, 100.0),
    ("blancs", 0.0, -100.0, 100.0),
    ("noirs", 0.0, -100.0, 100.0),
    ("saturation", 100.0, 0.0, 200.0),
    ("vibrance", 0.0, -100.0, 100.0),
    ("intensite", 100.0, 0.0, 100.0),
    ("vignette", 0.0, 0.0, 100.0),
];
pub const DUR_MIN: f64 = 0.2;
pub const DUR_MAX: f64 = 1.5;
pub const DUR_DEFAULT: f64 = 0.5;

pub fn couleur_path() -> PathBuf {
    paths::work().join("couleur.json")
}
pub fn transitions_path() -> PathBuf {
    paths::work().join("transitions.json")
}

fn f(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Réglages de couleur complets (défauts pour les clés absentes ou invalides).
pub fn color_settings(src: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (k, d, lo, hi) in COLOR_KEYS {
        let v = f(src.get(k)).filter(|x| x.is_finite()).unwrap_or(d).clamp(lo, hi);
        out.insert(k.into(), util::num(util::round_to(v, 2)));
    }
    let look = src.get("look").and_then(Value::as_str).filter(|l| LOOKS.contains(l)).unwrap_or("");
    out.insert("look".into(), json!(look));
    out
}

/// Couleurs sans effet (vignette comprise) : rien à appliquer.
pub fn color_neutral(c: &Map<String, Value>) -> bool {
    let s = color_settings(c);
    COLOR_KEYS.iter().all(|(k, d, ..)| *k == "intensite" || f(s.get(*k)) == Some(*d)) && s["look"] == json!("")
}

pub fn couleur() -> Map<String, Value> {
    color_settings(&util::read_obj(couleur_path()))
}

/// Enregistre les clés reçues (null : valeur par défaut) ; renvoie les réglages complets.
pub fn save_couleur(body: &Map<String, Value>) -> Map<String, Value> {
    let mut cur = util::read_obj(couleur_path());
    for (k, v) in body {
        if k == "look" || COLOR_KEYS.iter().any(|c| c.0 == k) {
            if v.is_null() {
                cur.remove(k);
            } else {
                cur.insert(k.clone(), v.clone());
            }
        }
    }
    if body.get("reset") == Some(&json!(true)) {
        cur = Map::new();
    }
    let s = color_settings(&cur);
    util::mkdirs(paths::work());
    let _ = util::write_json(couleur_path(), &Value::Object(s.clone()), Some(1));
    s
}

/// Une transition valide : {"kind", "dur"} (kind vide : coupe franche), ou None si invalide.
pub fn transition_of(v: &Value) -> Option<Map<String, Value>> {
    let o = v.as_object()?;
    let kind = o.get("kind").and_then(Value::as_str).unwrap_or("");
    if !kind.is_empty() && !TRANSITIONS.contains(&kind) {
        return None;
    }
    let dur = f(o.get("dur")).filter(|x| x.is_finite()).unwrap_or(DUR_DEFAULT).clamp(DUR_MIN, DUR_MAX);
    let mut m = Map::new();
    m.insert("kind".into(), json!(kind));
    m.insert("dur".into(), util::num(util::round_to(dur, 2)));
    Some(m)
}

pub fn transitions() -> Map<String, Value> {
    transition_of(&Value::Object(util::read_obj(transitions_path())))
        .unwrap_or_else(|| transition_of(&json!({"kind": ""})).unwrap())
}

pub fn save_transitions(body: &Map<String, Value>) -> Option<Map<String, Value>> {
    let mut cur = transitions();
    for (k, v) in body {
        cur.insert(k.clone(), v.clone());
    }
    let t = transition_of(&Value::Object(cur))?;
    util::mkdirs(paths::work());
    let _ = util::write_json(transitions_path(), &Value::Object(t.clone()), Some(1));
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    /// Les listes d'ici sont celles de selection/hf_look.js (même ordre).
    #[test]
    fn listes_comme_hf_look() {
        let js = std::fs::read_to_string(paths::root().join("selection").join("hf_look.js")).unwrap();
        let ids = |name: &str| -> Vec<String> {
            let start = js.find(&format!("const {name} = [")).unwrap();
            let end = start + js[start..].find("\n  ];").unwrap();
            let re = Regex::new(r#"\{ id: "([a-z]+)""#).unwrap();
            re.captures_iter(&js[start..end]).map(|c| c[1].to_string()).collect()
        };
        assert_eq!(ids("TRANSITIONS"), TRANSITIONS.to_vec());
        assert_eq!(ids("EFFECTS"), EFFECTS.to_vec());
        assert_eq!(ids("LOOKS"), LOOKS.to_vec());
    }

    #[test]
    fn reglages_bornes() {
        let s = color_settings(json!({"contraste": 250, "exposition": "-1.234", "look": "inconnu", "saturation": null}).as_object().unwrap());
        assert_eq!(s["contraste"].as_f64(), Some(100.0));
        assert_eq!(s["exposition"].as_f64(), Some(-1.23));
        assert_eq!(s["look"], json!(""));
        assert_eq!(s["saturation"].as_f64(), Some(100.0));
        assert!(color_neutral(&Map::new()));
        assert!(!color_neutral(json!({"vignette": 20}).as_object().unwrap()));
        assert!(color_neutral(json!({"intensite": 40}).as_object().unwrap()));
        assert_eq!(transition_of(&json!({"kind": "iris", "dur": 9})).unwrap()["dur"].as_f64(), Some(1.5));
        assert!(transition_of(&json!({"kind": "rien"})).is_none());
    }
}
