//! Petits outils communs : arrondis à la façon de Python, lecture et écriture de JSON, fichiers.

use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::Path;

/// `round(x, nd)` de Python (arrondi exact, moitié au pair).
pub fn round_to(x: f64, nd: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{:.*}", nd, x).parse().unwrap_or(x)
}

/// `round(x)` de Python (entier, moitié au pair).
pub fn round_int(x: f64) -> i64 {
    if !x.is_finite() {
        return 0;
    }
    format!("{:.0}", x).parse::<f64>().unwrap_or(0.0) as i64
}

/// `json.load(open(path))`, ou None si le fichier manque ou est illisible.
pub fn read_json(path: impl AsRef<Path>) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Objet JSON d'un fichier (dictionnaire vide sinon).
pub fn read_obj(path: impl AsRef<Path>) -> Map<String, Value> {
    match read_json(path) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// `json.dump(v, open(path, "w"), indent=indent)` (ensure_ascii comme Python).
pub fn write_json(path: impl AsRef<Path>, v: &Value, indent: Option<usize>) -> io::Result<()> {
    let text = crate::pyjson::dumps_with(v, crate::pyjson::Opts { indent, ..Default::default() });
    fs::write(path, text)
}

/// Écrit sous un nom provisoire puis renomme (pas de fichier à moitié écrit).
pub fn write_json_atomic(path: impl AsRef<Path>, v: &Value, indent: Option<usize>) -> io::Result<()> {
    let path = path.as_ref();
    let part = path.with_file_name(format!("{}.part", path.file_name().unwrap().to_string_lossy()));
    write_json(&part, v, indent)?;
    fs::rename(&part, path)
}

/// Texte d'un fichier sans espaces autour, ou None.
pub fn read_text(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

pub fn f(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

pub fn fd(v: &Value, key: &str, default: f64) -> f64 {
    f(v, key).unwrap_or(default)
}

pub fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

pub fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[])
}

/// Vérité à la façon de Python (`bool(x)`).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|x| x != 0.0).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Nombre JSON : entier s'il l'est (comme Python garde `int`), flottant sinon.
pub fn num(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

pub fn mkdirs(path: impl AsRef<Path>) {
    let _ = fs::create_dir_all(path);
}

/// `shutil.rmtree(path, ignore_errors=True)` ou suppression d'un fichier.
pub fn remove_all(path: impl AsRef<Path>) {
    let path = path.as_ref();
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            let _ = fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = fs::remove_file(path);
        }
        Err(_) => {}
    }
}

/// Noms des entrées d'un dossier, triés (vide si absent).
pub fn list_dir(path: impl AsRef<Path>) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(path)
        .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Date de modification en secondes (flottant), comme `os.path.getmtime`.
pub fn mtime(path: impl AsRef<Path>) -> Option<f64> {
    let m = fs::metadata(path).ok()?.modified().ok()?;
    let d = m.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_secs_f64())
}

/// Date de modification en nanosecondes, comme `os.stat().st_mtime_ns`.
pub fn mtime_ns(path: impl AsRef<Path>) -> Option<u128> {
    let m = fs::metadata(path).ok()?.modified().ok()?;
    Some(m.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos())
}

pub fn now_secs() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Lien physique, sinon copie (`os.link` avec repli sur `shutil.copy`).
pub fn link_or_copy(src: &Path, dst: &Path) -> io::Result<()> {
    let _ = fs::remove_file(dst);
    fs::hard_link(src, dst).or_else(|_| fs::copy(src, dst).map(|_| ()))
}

/// Texte français d'un nombre à virgule (« 1,5 »), comme les `str.replace(".", ",")` du Python.
pub fn comma(text: String) -> String {
    text.replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_like_python() {
        assert_eq!(round_to(0.125, 2), 0.12);
        assert_eq!(round_to(2.675, 2), 2.67);
        assert_eq!(round_int(2.5), 2);
        assert_eq!(round_int(3.5), 4);
        assert_eq!(round_int(-0.5), 0);
    }
}

/// Format `%.<prec>g` de Python (chiffres significatifs, zéros finaux retirés).
pub fn fmt_g(x: f64, prec: usize) -> String {
    if x == 0.0 {
        return "0".into();
    }
    if !x.is_finite() {
        return if x.is_nan() { "nan".into() } else if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let p = prec.max(1);
    let e = format!("{:.*e}", p - 1, x);
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let strip = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if exp < -4 || exp >= p as i32 {
        format!("{}e{}{:02}", strip(mant), if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        strip(&format!("{:.*}", decimals, x))
    }
}

#[cfg(test)]
mod tests_g {
    use super::fmt_g;
    #[test]
    fn g_like_python() {
        assert_eq!(fmt_g(12.5, 6), "12.5");
        assert_eq!(fmt_g(300.0, 6), "300");
        assert_eq!(fmt_g(1234567.0, 6), "1.23457e+06");
        assert_eq!(fmt_g(0.00001, 6), "1e-05");
        assert_eq!(fmt_g(25.8466666, 6), "25.8467");
    }
}
