//! Polices utilisables pour les sous-titres, et style choisi.
//!
//! Polices proposées (celles que le rendu peut vraiment utiliser) :
//!   - « Auto-montage » : public/fonts/ (Oliver, Patrick Hand, Poppins…) ;
//!   - « Vos polices »  : public/fonts/perso/, ajoutées depuis la page des moments ;
//!   - « Système »      : polices installées là où tourne le rendu (dossiers de polices du
//!                        système de la machine).
//! Formats lisibles par le navigateur de rendu seulement (ttf, otf, woff, woff2), sans italique.
//! Famille, graisse et italique sont lus dans le fichier de la police (tables name et OS/2).
//!
//! Style des sous-titres : work/caption_style.json (montage en cours ; mis de côté et rouvert
//! avec lui), et work/caption_style_default.json (dernier style choisi, repris par les montages
//! suivants). Forme : {"font": "<id de famille>", "weight": 700, "size": 64,
//! "uppercase": true, "color": "#f4e2c2", "shadow": 50, "blur": 14, "ox": 0, "oy": 0} (ombre :
//! intensité de 0 à 100 (plus ou moins sombre), diffusion du flou en pixels de 0 à 60, et décalage
//! ox (vers la droite) et oy (vers le bas) en pixels, à 40 px au plus du texte, choisi dans le cercle
//! de la page des moments ; tout en pixels sur une image de 1080 px de large), "lines": 2, "lineSizes": [100, …]
//! (nombre de lignes voulu, de 1 à 6 : moins pour un sous-titre qui tient sur une ligne, et taille de
//! chaque ligne en % de la première, de 50 à 200 ; réglés sur la page des moments et sur « Rushes et
//! montages »), "posX": 0, "posY": 0 (position : décalage depuis la place d'origine, en % de la
//! moitié de l'image, dans un cercle de rayon 100, choisi dans le cercle de la page des moments ; le
//! sous-titre reste dans l'image). build_edit le copie dans edit.json
//! (« captionStyle ») avec les fichiers de la police, copiés dans public/fonts/choisie/.

use crate::paths;
use crate::util::{self, round_int};
use regex::Regex;
use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub fn fonts_dir() -> PathBuf {
    paths::public().join("fonts")
}
pub fn perso_dir() -> PathBuf {
    fonts_dir().join("perso")
}
pub fn chosen_dir() -> PathBuf {
    fonts_dir().join("choisie")
}
pub fn style_path() -> PathBuf {
    paths::work().join("caption_style.json")
}
pub fn style_default_path() -> PathBuf {
    paths::work().join("caption_style_default.json")
}

pub const FONT_EXT: [&str; 4] = [".ttf", ".otf", ".woff", ".woff2"];

/// Style d'origine : Oliver (ou Patrick Hand) en gras, crème, en majuscules.
pub fn default_style() -> Map<String, Value> {
    match json!({"font": null, "weight": 700, "size": 64, "uppercase": true, "color": "#f4e2c2", "shadow": 50, "blur": 14,
                 "ox": 0, "oy": 0, "lines": 2, "lineSizes": vec![100; LINES_MAX], "posX": 0, "posY": 0}) {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

/// Lignes d'un sous-titre au plus : au-delà, le choix des coupures (toutes les façons essayées,
/// render_lib::caption_layout) deviendrait trop long.
pub const LINES_MAX: usize = 6;
pub const LINES_RANGE: (i64, i64) = (1, LINES_MAX as i64);
pub const LINE_SIZE_RANGE: (i64, i64) = (50, 200);
/// Mots et lettres au plus par sous-titre, selon le nombre de lignes : 3 mots et 12 lettres de plus
/// par ligne (1 ligne : 3 mots, 14 lettres ; 2 : 6 et 26 ; 3 : 9 et 38). Même règle dans la page des
/// moments (captionLimits).
pub fn caption_limits_for(lines: i64) -> (i64, i64) {
    let l = lines.clamp(LINES_RANGE.0, LINES_RANGE.1);
    (3 * l, 12 * l + 2)
}
/// Décalage de la position des sous-titres au plus (posX, posY), en % de la moitié de l'image.
pub const POS_MAX: f64 = 100.0;
pub const SHADOW_RANGE: (i64, i64) = (0, 100);
pub const BLUR_RANGE: (i64, i64) = (0, 60);
pub const OFFSET_MAX: f64 = 40.0;
pub const SIZE_RANGE: (i64, i64) = (36, 120);

fn has_font_ext(name: &str) -> bool {
    let l = name.to_lowercase();
    FONT_EXT.iter().any(|e| l.ends_with(e))
}

/// Famille et graisse d'après le nom du fichier (police illisible).
pub fn guess_from_name(path: &Path) -> (String, i64, bool) {
    static SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[-_](latin|subset)$").unwrap());
    static WEIGHT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^(.*?)[-_ ]?(thin|extralight|light|regular|medium|semibold|extrabold|bold|black|heavy)$").unwrap()
    });
    static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([a-z])([A-Z])").unwrap());
    let stem0 = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = SUFFIX.replace(&stem0, "").into_owned();
    let mut weight = 400;
    let mut family = stem.clone();
    if let Some(m) = WEIGHT.captures(&stem) {
        family = m[1].to_string();
        weight = match m[2].to_lowercase().as_str() {
            "thin" => 100,
            "extralight" => 200,
            "light" => 300,
            "regular" => 400,
            "medium" => 500,
            "semibold" => 600,
            "extrabold" => 800,
            "bold" => 700,
            _ => 900,
        };
    }
    let family = CAMEL.replace_all(&family, "$1 $2").replace(['_', '-'], " ").trim().to_string();
    let low = stem.to_lowercase();
    (if family.is_empty() { stem } else { family }, weight, low.contains("italic") || low.contains("oblique"))
}

/// Données de la police en TrueType/OpenType (woff2 décompressé).
pub fn font_bytes(path: &Path) -> Option<Vec<u8>> {
    let data = fs::read(path).ok()?;
    if data.starts_with(b"wOF2") {
        return woff2_patched::convert_woff2_to_ttf(&mut data.as_slice()).ok();
    }
    if data.starts_with(b"wOFF") {
        return woff1_to_sfnt(&data);
    }
    Some(data)
}

/// WOFF 1 : tables décompressées (zlib) et remises dans un fichier OpenType.
fn woff1_to_sfnt(data: &[u8]) -> Option<Vec<u8>> {
    let be32 = |o: usize| -> Option<u32> { Some(u32::from_be_bytes(data.get(o..o + 4)?.try_into().ok()?)) };
    let be16 = |o: usize| -> Option<u16> { Some(u16::from_be_bytes(data.get(o..o + 2)?.try_into().ok()?)) };
    let flavor = be32(4)?;
    let num = be16(12)? as usize;
    let mut tables = vec![];
    for i in 0..num {
        let o = 44 + i * 20;
        let (tag, off, clen, olen, sum) = (be32(o)?, be32(o + 4)? as usize, be32(o + 8)? as usize, be32(o + 12)? as usize, be32(o + 16)?);
        let raw = data.get(off..off + clen)?;
        let bytes = if clen < olen {
            let mut out = Vec::with_capacity(olen);
            use std::io::Read;
            flate2::read::ZlibDecoder::new(raw).read_to_end(&mut out).ok()?;
            out
        } else {
            raw.to_vec()
        };
        tables.push((tag, sum, bytes));
    }
    let mut out = vec![];
    out.extend(flavor.to_be_bytes());
    out.extend((num as u16).to_be_bytes());
    let mut pow = 1u16;
    let mut log = 0u16;
    while (pow as usize) * 2 <= num {
        pow *= 2;
        log += 1;
    }
    out.extend((pow * 16).to_be_bytes());
    out.extend(log.to_be_bytes());
    out.extend((num as u16 * 16 - pow * 16).to_be_bytes());
    let mut offset = 12 + 16 * num;
    let mut body = vec![];
    for (tag, sum, bytes) in &tables {
        out.extend(tag.to_be_bytes());
        out.extend(sum.to_be_bytes());
        out.extend((offset as u32).to_be_bytes());
        out.extend((bytes.len() as u32).to_be_bytes());
        body.extend(bytes);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * num + body.len();
    }
    out.extend(body);
    Some(out)
}

fn name_of(face: &ttf_parser::Face, id: u16) -> Option<String> {
    let names: Vec<_> = face.names().into_iter().filter(|n| n.name_id == id).collect();
    // Anglais (Windows, puis Mac) d'abord, comme fontconfig.
    let pick = names
        .iter()
        .find(|n| n.is_unicode() && n.language_id == 0x0409)
        .or_else(|| names.iter().find(|n| n.is_unicode()))
        .or_else(|| names.first())?;
    pick.to_string().or_else(|| Some(String::from_utf8_lossy(pick.name).into_owned())).filter(|s| !s.is_empty())
}

/// Graisse CSS (100 à 900) d'après la classe de graisse de la table OS/2.
fn css_weight(class: u16) -> i64 {
    let w = if class < 10 { class as i64 * 100 } else { class as i64 };
    ((w as f64 / 100.0).round() as i64 * 100).clamp(100, 900)
}

/// (famille, graisse CSS, italique) d'un fichier, lus dans la police si possible.
pub fn query(path: &Path) -> (String, i64, bool) {
    let Some(data) = font_bytes(path) else { return guess_from_name(path) };
    let Ok(face) = ttf_parser::Face::parse(&data, 0) else { return guess_from_name(path) };
    let Some(family) = name_of(&face, 1).or_else(|| name_of(&face, 16)) else { return guess_from_name(path) };
    let weight = css_weight(face.weight().to_number());
    let italic = face.is_italic() || face.is_oblique() || face.italic_angle() != 0.0;
    (family, weight, italic)
}

pub fn family_id(source: &str, family: &str) -> String {
    static NON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());
    let s = NON.replace_all(&format!("{source} {family}").to_lowercase(), "-").trim_matches('-').to_string();
    s.chars().take(60).collect()
}

pub fn file_id(path: &Path) -> String {
    let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut h = Sha1::new();
    h.update(real.to_string_lossy().as_bytes());
    hex::encode(h.finalize())[..16].to_string()
}

/// Famille de polices : identifiant, nom, origine et fichier de chaque graisse.
#[derive(Clone, Debug)]
pub struct Family {
    pub id: String,
    pub family: String,
    pub source: String,
    pub faces: BTreeMap<i64, PathBuf>,
}

impl Family {
}

fn add(families: &mut Vec<Family>, source: &str, label: &str, path: &Path, family: &str, weight: i64) {
    let fid = family_id(source, family);
    let pos = match families.iter().position(|f| f.id == fid) {
        Some(p) => p,
        None => {
            families.push(Family { id: fid, family: family.to_string(), source: label.to_string(), faces: BTreeMap::new() });
            families.len() - 1
        }
    };
    // une seule face par graisse (la première trouvée)
    families[pos].faces.entry(weight).or_insert_with(|| path.to_path_buf());
}

/// Dossiers de polices du système (ceux que fontconfig, CoreText ou Windows parcourent).
pub fn system_font_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = vec![];
    if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(PathBuf::from));
        if let Some(h) = &home {
            dirs.push(h.join("Library/Fonts"));
        }
    } else if cfg!(windows) {
        let windir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        dirs.push(windir.join("Fonts"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
        }
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
        if let Some(h) = &home {
            dirs.push(h.join(".local/share/fonts"));
            dirs.push(h.join(".fonts"));
        }
    }
    dirs
}

/// Familles utilisables, triées : Auto-montage, Vos polices, Système, puis par nom.
pub fn list_fonts() -> Vec<Family> {
    let mut families: Vec<Family> = vec![];
    for (folder, source, label) in [(fonts_dir(), "am", "Auto-montage"), (perso_dir(), "perso", "Vos polices")] {
        for name in util::list_dir(&folder) {
            let path = folder.join(&name);
            if path.is_file() && has_font_ext(&name) && !name.starts_with('.') {
                let (family, weight, italic) = query(&path);
                if !italic {
                    add(&mut families, source, label, &path, &family, weight);
                }
            }
        }
    }
    let fonts = fonts_dir();
    let mut lines: Vec<(String, i64, bool, PathBuf)> = vec![];
    for dir in system_font_dirs() {
        for e in walkdir::WalkDir::new(&dir).follow_links(true).into_iter().flatten() {
            let p = e.path();
            if !e.file_type().is_file() || !has_font_ext(&p.to_string_lossy()) || p.starts_with(&fonts) {
                continue;
            }
            let (family, weight, italic) = query(p);
            lines.push((family, weight, italic, p.to_path_buf()));
        }
    }
    lines.sort_by(|a, b| (a.0.as_str(), a.1, a.2, &a.3).cmp(&(b.0.as_str(), b.1, b.2, &b.3)));
    for (family, weight, italic, path) in lines {
        if !italic {
            add(&mut families, "sys", "Système", &path, &family, weight);
        }
    }
    let order = |s: &str| match s {
        "Auto-montage" => 0,
        "Vos polices" => 1,
        _ => 2,
    };
    families.sort_by(|a, b| (order(&a.source), a.family.to_lowercase()).cmp(&(order(&b.source), b.family.to_lowercase())));
    families
}

/// Police d'origine des sous-titres : Oliver si présente, sinon Patrick Hand.
pub fn default_font_id(fonts: &[Family]) -> Option<String> {
    for wanted in ["oliver", "patrick hand"] {
        if let Some(f) = fonts.iter().find(|f| f.source == "Auto-montage" && f.family.to_lowercase().starts_with(wanted)) {
            return Some(f.id.clone());
        }
    }
    fonts.first().map(|f| f.id.clone())
}

pub fn read_style() -> Map<String, Value> {
    for path in [style_path(), style_default_path()] {
        if let Some(Value::Object(data)) = util::read_json(&path) {
            return clean(&data);
        }
    }
    default_style()
}

fn as_f(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn clamp(x: i64, r: (i64, i64)) -> i64 {
    x.clamp(r.0, r.1)
}

/// Couleur « #rrggbb » : seule forme acceptée des couleurs lues dans work/ (elles partent dans le
/// style des pages : rien d'autre ne doit pouvoir s'y glisser).
pub fn is_color(c: &str) -> bool {
    static COLOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#[0-9a-fA-F]{6}$").unwrap());
    COLOR.is_match(c)
}

pub fn clean(data: &Map<String, Value>) -> Map<String, Value> {
    static FONT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9-]{1,60}$").unwrap());
    let mut style = default_style();
    if let Some(f) = data.get("font").and_then(Value::as_str) {
        if FONT.is_match(f) {
            style.insert("font".into(), json!(f));
        }
    }
    let weight = if data.contains_key("weight") { as_f(data.get("weight")) } else { Some(700.0) };
    let size = if data.contains_key("size") { as_f(data.get("size")) } else { Some(64.0) };
    if let (Some(w), Some(s)) = (weight, size) {
        style.insert("weight".into(), json!((round_int(w / 100.0) * 100).clamp(100, 900)));
        style.insert("size".into(), json!(clamp(round_int(s), SIZE_RANGE)));
    } else if let Some(w) = weight {
        style.insert("weight".into(), json!((round_int(w / 100.0) * 100).clamp(100, 900)));
    }
    style.insert("uppercase".into(), json!(data.get("uppercase").map(|v| util::truthy(Some(v))).unwrap_or(true)));
    if let Some(c) = data.get("color").and_then(Value::as_str) {
        if is_color(c) {
            style.insert("color".into(), json!(c.to_lowercase()));
        }
    }
    for (key, range) in [("shadow", SHADOW_RANGE), ("blur", BLUR_RANGE)] {
        let v = if data.contains_key(key) { as_f(data.get(key)) } else { style[key].as_f64() };
        if let Some(v) = v {
            style.insert(key.into(), json!(clamp(round_int(v), range)));
        }
    }
    let mut merged = data.clone();
    merged.insert("shadow".into(), style["shadow"].clone());
    merged.insert("blur".into(), style["blur"].clone());
    let (ox, oy) = shadow_offset(&merged);
    style.insert("ox".into(), json!(ox));
    style.insert("oy".into(), json!(oy));
    let lines = match data.get("lines") {
        None => Some(2),
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        Some(Value::String(s)) => s.trim().parse().ok(),
        Some(Value::Bool(b)) => Some(*b as i64),
        _ => None,
    };
    if let Some(l) = lines {
        style.insert("lines".into(), json!(clamp(l, LINES_RANGE)));
    }
    // Une taille par ligne possible (la première à 100 %), même au-delà du nombre de lignes choisi :
    // revenir à plus de lignes retrouve les tailles d'avant.
    if let Some(Value::Array(sizes)) = data.get("lineSizes") {
        let parsed: Option<Vec<i64>> =
            sizes.iter().take(LINES_MAX).map(|x| as_f(Some(x)).map(|f| clamp(round_int(f), LINE_SIZE_RANGE))).collect();
        if let Some(sizes) = parsed {
            let mut v = vec![100i64];
            v.extend(sizes.iter().skip(1).copied());
            v.resize(LINES_MAX, 100);
            style.insert("lineSizes".into(), json!(v));
        }
    }
    let (px, py) = caption_position(data);
    style.insert("posX".into(), json!(px));
    style.insert("posY".into(), json!(py));
    style
}

/// (mots, lettres) au plus par sous-titre.
pub fn caption_limits(style: &Map<String, Value>) -> (i64, i64) {
    caption_limits_for(style.get("lines").and_then(Value::as_i64).unwrap_or(2))
}

/// Position des sous-titres (posX vers la droite, posY vers le bas, en % de la moitié de l'image),
/// dans le cercle de rayon POS_MAX ; (0, 0) : place d'origine.
pub fn caption_position(style: &Map<String, Value>) -> (i64, i64) {
    let num = |k: &str| as_f(style.get(k)).filter(|v| v.is_finite()).unwrap_or(0.0);
    let (mut x, mut y) = (num("posX"), num("posY"));
    let r = (x * x + y * y).sqrt();
    if r > POS_MAX {
        x = x * POS_MAX / r;
        y = y * POS_MAX / r;
    }
    (round_int(x), round_int(y))
}

/// Décalage de l'ombre (ox, oy) en pixels, dans le cercle de rayon OFFSET_MAX.
pub fn shadow_offset(style: &Map<String, Value>) -> (i64, i64) {
    let num = |k: &str| -> Option<f64> {
        match style.get(k) {
            None | Some(Value::Null) => Some(0.0),
            Some(v) if !util::truthy(Some(v)) => Some(0.0),
            v => as_f(v),
        }
    };
    let (Some(mut ox), Some(mut oy)) = (num("ox"), num("oy")) else { return (0, 0) };
    let r = (ox * ox + oy * oy).sqrt();
    if r > OFFSET_MAX {
        ox = ox * OFFSET_MAX / r;
        oy = oy * OFFSET_MAX / r;
    }
    (round_int(ox), round_int(oy))
}

/// Enregistre le style ; les clés absentes gardent leur valeur (la page « Rushes et montages »
/// n'envoie que les lignes).
pub fn save_style(data: &Map<String, Value>) -> Map<String, Value> {
    let mut base = read_style();
    for (k, v) in data {
        base.insert(k.clone(), v.clone());
    }
    let style = clean(&base);
    util::mkdirs(paths::work());
    for path in [style_path(), style_default_path()] {
        let _ = util::write_json(path, &Value::Object(style.clone()), Some(1));
    }
    style
}

/// Famille du style (ou celle d'origine si elle n'existe plus sur cette machine).
pub fn resolve(style: &Map<String, Value>, fonts: Option<&[Family]>) -> Option<Family> {
    let owned;
    let fonts = match fonts {
        Some(f) => f,
        None => {
            owned = list_fonts();
            &owned
        }
    };
    let want = style.get("font").and_then(Value::as_str);
    if let Some(f) = want.and_then(|id| fonts.iter().find(|f| f.id == id)) {
        return Some(f.clone());
    }
    let d = default_font_id(fonts)?;
    fonts.iter().find(|f| f.id == d).cloned()
}

/// Face à charger : la plus proche de la graisse voulue (un gras absent de la police est
/// simulé par le navigateur).
pub fn faces_for(family: &Family, weight: i64) -> Vec<(i64, PathBuf)> {
    let near = family.faces.keys().copied().min_by_key(|w| ((w - weight).abs(), *w));
    near.map(|w| vec![(w, family.faces[&w].clone())]).unwrap_or_default()
}

/// Copie la police choisie dans public/fonts/choisie/ : {"family", "files", …} pour edit.json.
pub fn copy_for_render(style: &Map<String, Value>) -> Option<Value> {
    let family = resolve(style, None);
    util::remove_all(chosen_dir());
    let family = family?;
    util::mkdirs(chosen_dir());
    let weight = style.get("weight").and_then(Value::as_i64).unwrap_or(700);
    let mut files = vec![];
    for (w, path) in faces_for(&family, weight) {
        let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default();
        let name = format!("{w}{ext}");
        let _ = fs::copy(&path, chosen_dir().join(&name));
        files.push(json!({"url": format!("fonts/choisie/{name}"), "weight": w.to_string()}));
    }
    // Nom de famille propre au rendu : pas de confusion avec une police du système du même nom.
    let g = |k: &str| style.get(k).cloned().unwrap_or(Value::Null);
    Some(json!({"family": "AM Sous-titres", "label": family.family, "files": files, "weight": g("weight"), "size": g("size"),
                "uppercase": g("uppercase"), "color": g("color"), "shadow": g("shadow"), "blur": g("blur"),
                "ox": g("ox"), "oy": g("oy"), "lines": g("lines"), "lineSizes": g("lineSizes"),
                "posX": g("posX"), "posY": g("posY")}))
}

/// `auto-montage fonts_lib` : polices utilisables et style actuel (pour Claude et le dépannage).
pub fn main(_args: &[String]) -> i32 {
    for f in list_fonts() {
        let ws: Vec<String> = f.faces.keys().map(|w| w.to_string()).collect();
        println!("{:32} {} ({}) graisses {}", f.id, f.family, f.source, ws.join(", "));
    }
    println!("Style actuel : {}", crate::pyjson::dumps_utf8(&Value::Object(read_style())));
    0
}
