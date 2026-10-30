//! Retouche des couleurs, effets d'image et transitions (repris de FilmCraft) : réglages et
//! listes côté programme. Le dessin est fait par selection/hf_look.js, le même code pour le rendu
//! (render_hyperframes, page HyperFrames) et pour l'aperçu de la page des moments.
//!
//!   work/couleur.json      couleurs de tout le montage : température, teinte, exposition (IL),
//!                          contraste, hautes, ombres, blancs, noirs (-100 à 100), saturation (%),
//!                          vibrance, look (préréglage créatif) et son intensité (%), vignette (%).
//!                          Le look est reporté sur les curseurs (`LOOK_PARAMS`, `with_look`) :
//!                          le choisir ou changer son intensité les déplace, et il ne garde à
//!                          part que son virage partiel. `"v": 2` le note ; un ancien fichier sans
//!                          lui (look ajouté au rendu) est converti à la lecture (`color_settings`).
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
/// Écarts de chaque look sur les curseurs (LOOKS de selection/hf_look.js, champ « p » ; saturation
/// en points de %), à intensité 100 %.
pub const LOOK_PARAMS: [(&str, &[(&str, f64)]); 8] = [
    ("chaud", &[("temperature", 30.0), ("saturation", 6.0), ("ombres", 5.0)]),
    ("froid", &[("temperature", -30.0), ("teinte", -6.0)]),
    ("cinema", &[("contraste", 18.0), ("saturation", -4.0), ("hautes", -8.0)]),
    ("vintage", &[("noirs", 30.0), ("blancs", -12.0), ("saturation", -22.0), ("temperature", 18.0)]),
    ("delave", &[("noirs", 35.0), ("contraste", -18.0), ("saturation", -15.0)]),
    ("vif", &[("vibrance", 45.0), ("contraste", 10.0), ("saturation", 8.0)]),
    ("nb", &[("saturation", -100.0), ("contraste", 28.0), ("noirs", -10.0)]),
    ("nuit", &[("exposition", -0.9), ("temperature", -45.0), ("saturation", -30.0), ("contraste", 10.0)]),
];

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

fn look_param(look: &str, key: &str) -> f64 {
    LOOK_PARAMS.iter().find(|l| l.0 == look).and_then(|l| l.1.iter().find(|p| p.0 == key)).map_or(0.0, |p| p.1)
}

/// Réglages de couleur complets (défauts pour les clés absentes ou invalides).
pub fn color_settings(src: &Map<String, Value>) -> Map<String, Value> {
    let look = src.get("look").and_then(Value::as_str).filter(|l| LOOKS.contains(l)).unwrap_or("");
    // Ancien fichier : le look s'ajoutait aux curseurs au rendu (saturation multipliée).
    let old = src.get("v") != Some(&json!(2));
    let k = f(src.get("intensite")).filter(|x| x.is_finite()).unwrap_or(100.0).clamp(0.0, 100.0) / 100.0;
    let mut out = Map::new();
    for (key, d, lo, hi) in COLOR_KEYS {
        let mut v = f(src.get(key)).filter(|x| x.is_finite()).unwrap_or(d);
        if old && key == "saturation" {
            v *= (1.0 + k * look_param(look, key) / 100.0).max(0.0);
        } else if old && key != "intensite" {
            v += k * look_param(look, key);
        }
        out.insert(key.into(), util::num(util::round_to(v.clamp(lo, hi), 2)));
    }
    out.insert("look".into(), json!(look));
    out.insert("v".into(), json!(2));
    out
}

/// Réglages avec un autre look ou une autre intensité : l'écart de l'ancien look est retiré des
/// curseurs, celui du nouveau ajouté (les retouches faites à la main restent).
pub fn with_look(c: &Map<String, Value>, look: &str, intensite: f64) -> Map<String, Value> {
    let mut s = color_settings(c);
    let look = if LOOKS.contains(&look) { look } else { "" };
    let k_new = intensite.clamp(0.0, 100.0) / 100.0;
    let old = s["look"].as_str().unwrap_or("").to_string();
    let k_old = f(s.get("intensite")).unwrap_or(100.0) / 100.0;
    for (key, _, lo, hi) in COLOR_KEYS {
        if key == "intensite" {
            continue;
        }
        let v = f(s.get(key)).unwrap_or(0.0) - k_old * look_param(&old, key) + k_new * look_param(look, key);
        s.insert(key.into(), util::num(util::round_to(v.clamp(lo, hi), 2)));
    }
    s.insert("look".into(), json!(look));
    s.insert("intensite".into(), util::num(util::round_to(intensite.clamp(0.0, 100.0), 2)));
    s
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
/// Look ou intensité reçus sans aucun curseur : les curseurs suivent le look (`with_look`) ;
/// avec des curseurs (réglages complets, annuler), tout est pris tel quel.
pub fn save_couleur(body: &Map<String, Value>) -> Map<String, Value> {
    let mut cur = color_settings(&util::read_obj(couleur_path()));
    let sliders = COLOR_KEYS.iter().any(|c| c.0 != "intensite" && body.contains_key(c.0));
    if !sliders && (body.contains_key("look") || body.contains_key("intensite")) {
        let look = match body.get("look") {
            Some(v) => v.as_str().unwrap_or(""),
            None => cur["look"].as_str().unwrap_or(""),
        }
        .to_string();
        let k = match body.get("intensite") {
            Some(Value::Null) => 100.0,
            Some(v) => f(Some(v)).filter(|x| x.is_finite()).unwrap_or(100.0),
            None => f(cur.get("intensite")).unwrap_or(100.0),
        };
        cur = with_look(&cur, &look, k);
    }
    for (k, v) in body {
        if sliders && (k == "look" || COLOR_KEYS.iter().any(|c| c.0 == k)) {
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
        // Écarts de chaque look (champ « p », virage partiel à part).
        let start = js.find("const LOOKS = [").unwrap();
        let end = start + js[start..].find("\n  ];").unwrap();
        let re = Regex::new(r#"\{ id: "([a-z]+)", label: "[^"]+", p: \{([^}]*)"#).unwrap();
        let pair = Regex::new(r"([a-z]+): (-?[0-9.]+)").unwrap();
        let js_params: Vec<(String, Vec<(String, f64)>)> = re
            .captures_iter(&js[start..end])
            .map(|c| (c[1].to_string(), pair.captures_iter(&c[2]).map(|p| (p[1].to_string(), p[2].parse().unwrap())).collect()))
            .collect();
        let rs: Vec<(String, Vec<(String, f64)>)> =
            LOOK_PARAMS.iter().map(|(l, p)| (l.to_string(), p.iter().map(|(k, v)| (k.to_string(), *v)).collect())).collect();
        assert_eq!(js_params, rs);
    }

    #[test]
    fn look_sur_les_curseurs() {
        let base = color_settings(json!({"exposition": 0.5, "temperature": 10}).as_object().unwrap());
        let chaud = with_look(&base, "chaud", 100.0);
        assert_eq!(chaud["temperature"].as_f64(), Some(40.0));
        assert_eq!(chaud["saturation"].as_f64(), Some(106.0));
        assert_eq!(chaud["exposition"].as_f64(), Some(0.5));
        let froid = with_look(&chaud, "froid", 50.0);
        assert_eq!(froid["temperature"].as_f64(), Some(-5.0));
        assert_eq!(froid["teinte"].as_f64(), Some(-3.0));
        assert_eq!(froid["saturation"].as_f64(), Some(100.0));
        assert_eq!(froid["ombres"].as_f64(), Some(0.0));
        let aucun = with_look(&froid, "", 100.0);
        assert_eq!(aucun["temperature"].as_f64(), Some(10.0));
        assert_eq!(aucun["look"], json!(""));
        // Ancien fichier : le look est reporté sur les curseurs à la lecture.
        let old = color_settings(json!({"look": "nuit", "intensite": 50, "saturation": 80}).as_object().unwrap());
        assert_eq!(old["temperature"].as_f64(), Some(-22.5));
        assert_eq!(old["exposition"].as_f64(), Some(-0.45));
        assert_eq!(old["saturation"].as_f64(), Some(68.0));
        assert_eq!(color_settings(&old), old);
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
