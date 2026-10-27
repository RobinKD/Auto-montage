//! JSON écrit exactement comme `json.dumps` de Python (séparateurs « , » et « : », flottants à la
//! façon de `repr`, `ensure_ascii`) : les pages et les tests lisent les fichiers de `work/` et les données insérées
//! dans les pages tels quels (ex. `"rush": "essai.mov"`).

use serde_json::Value;
use std::fmt::Write;

#[derive(Clone, Copy)]
pub struct Opts {
    pub indent: Option<usize>,
    pub ensure_ascii: bool,
    pub sort_keys: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { indent: None, ensure_ascii: true, sort_keys: false }
    }
}

/// `json.dumps(v)` (ensure_ascii par défaut, comme Python).
pub fn dumps(v: &Value) -> String {
    dumps_with(v, Opts::default())
}

/// `json.dumps(v, ensure_ascii=False)`.
pub fn dumps_utf8(v: &Value) -> String {
    dumps_with(v, Opts { ensure_ascii: false, ..Opts::default() })
}

/// `json.dumps(v, indent=n)`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn dumps_indent(v: &Value, indent: usize) -> String {
    dumps_with(v, Opts { indent: Some(indent), ..Opts::default() })
}

pub fn dumps_with(v: &Value, o: Opts) -> String {
    let mut s = String::new();
    write_value(&mut s, v, o, 0);
    s
}

/// `repr(float)` de Python : chiffres les plus courts, notation fixe entre 1e-4 et 1e16.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let e = format!("{:e}", x); // ex. « -1.2345e-5 »
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if (-4..16).contains(&exp) {
        if exp >= 0 {
            let int_len = (exp + 1) as usize;
            if digits.len() <= int_len {
                out.push_str(&digits);
                out.push_str(&"0".repeat(int_len - digits.len()));
                out.push_str(".0");
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            out.push_str(&"0".repeat((-exp - 1) as usize));
            out.push_str(&digits);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(out, "e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    out
}

fn write_str(s: &mut String, text: &str, ascii: bool) {
    s.push('"');
    for c in text.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            '\u{8}' => s.push_str("\\b"),
            '\u{c}' => s.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(s, "\\u{:04x}", c as u32);
            }
            c if ascii && (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    let _ = write!(s, "\\u{:04x}", u);
                }
            }
            c => s.push(c),
        }
    }
    s.push('"');
}

fn newline(s: &mut String, o: Opts, level: usize) {
    if let Some(n) = o.indent {
        s.push('\n');
        s.push_str(&" ".repeat(n * level));
    }
}

fn write_value(s: &mut String, v: &Value, o: Opts, level: usize) {
    let item_sep = if o.indent.is_some() { "," } else { ", " };
    match v {
        Value::Null => s.push_str("null"),
        Value::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                let _ = write!(s, "{}", i);
            } else if let Some(u) = n.as_u64() {
                let _ = write!(s, "{}", u);
            } else {
                s.push_str(&float_repr(n.as_f64().unwrap_or(0.0)));
            }
        }
        Value::String(t) => write_str(s, t, o.ensure_ascii),
        Value::Array(a) => {
            if a.is_empty() {
                s.push_str("[]");
                return;
            }
            s.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    s.push_str(item_sep);
                }
                newline(s, o, level + 1);
                write_value(s, x, o, level + 1);
            }
            newline(s, o, level);
            s.push(']');
        }
        Value::Object(m) => {
            if m.is_empty() {
                s.push_str("{}");
                return;
            }
            s.push('{');
            let mut keys: Vec<&String> = m.keys().collect();
            if o.sort_keys {
                keys.sort();
            }
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    s.push_str(item_sep);
                }
                newline(s, o, level + 1);
                write_str(s, k, o.ensure_ascii);
                s.push_str(": ");
                write_value(s, &m[k.as_str()], o, level + 1);
            }
            newline(s, o, level);
            s.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn floats_like_python() {
        assert_eq!(float_repr(1.0), "1.0");
        assert_eq!(float_repr(0.1), "0.1");
        assert_eq!(float_repr(1e-5), "1e-05");
        assert_eq!(float_repr(0.0001), "0.0001");
        assert_eq!(float_repr(1e16), "1e+16");
        assert_eq!(float_repr(123456789012345.0), "123456789012345.0");
        assert_eq!(float_repr(-2.5e-7), "-2.5e-07");
        assert_eq!(float_repr(12.345), "12.345");
    }

    #[test]
    fn dumps_like_python() {
        let v = json!({"a": 1, "b": [1.5, "é", null], "c": {}});
        assert_eq!(dumps(&v), r#"{"a": 1, "b": [1.5, "\u00e9", null], "c": {}}"#);
        assert_eq!(dumps_utf8(&v), r#"{"a": 1, "b": [1.5, "é", null], "c": {}}"#);
        assert_eq!(dumps_indent(&json!({"a": [1, 2]}), 1), "{\n \"a\": [\n  1,\n  2\n ]\n}");
    }
}
