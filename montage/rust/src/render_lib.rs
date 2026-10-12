//! Ce que le rendu (render_hyperframes) prépare avant la page HyperFrames : montage
//! (src/data/edit.json, face.json), navigateur de rendu, bruitages, police des sous-titres, mots
//! mis en valeur et coupure des sous-titres en lignes (mesurée dans la police). Les effets image
//! par image (zooms centrés sur le visage, secousses, flashs, ressort des apparitions) sont dans la
//! page HyperFrames, en JavaScript ; la page des moments les imite en direct.
//!
//! Unités : pixels CSS de l'image de la composition, 1080 x 1920 (portrait) ou 1920 x 1080 (rush en
//! paysage) ; le rendu 4K multiplie par 2.
//!
//! Mesure du texte : mise en forme HarfBuzz (rustybuzz), comme Pillow avec libraqm dans la version
//! Python (crénage compris), en 1/64 de pixel.

use crate::fonts_lib;
use crate::paths;
use crate::util::{self, read_json};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

pub const CAPTION_MARGIN: f64 = 140.0; // sous-titres : largeur de l'image moins cette marge (940 px sur 1080)
pub const HIGHLIGHT_SCALE: f64 = 1.2;

pub fn poppins() -> PathBuf {
    paths::public().join("fonts").join("Poppins-Black-latin.ttf")
}

/// Chrome sans écran : celui que trouve ou télécharge `auto-montage ensure_browser` (Chromium de
/// Playwright sur Linux ARM64, « hyperframes browser ensure » ailleurs).
pub fn headless_chrome() -> Option<PathBuf> {
    let (_, out, _) = crate::procs::run(paths::self_exe(), ["ensure_browser"]);
    // Le chemin du navigateur : dernier chemin affiché qui est un fichier.
    let paths: Vec<PathBuf> = out
        .lines()
        .flat_map(|l| l.split_whitespace())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .collect();
    paths.into_iter().rev().find(|p| p.is_file())
}

/// Mot comparé sans accents ni ponctuation (comme norm() de la page des moments).
pub fn norm(w: &str) -> String {
    static NON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());
    let base: String = w.to_lowercase().nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).collect();
    NON.replace_all(&base, "").into_owned()
}

/// Police des sous-titres : (fichier, graisse du fichier) pour la graisse demandée, comme le
/// navigateur (fichier le plus proche ; un gras absent est simulé). Sans style choisi : Oliver
/// (déclarée en 400, gras simulé), sinon Patrick Hand (déclarée en 700, sans gras simulé).
pub fn caption_font(cs: &Map<String, Value>) -> (PathBuf, i64) {
    let weight = cs.get("weight").and_then(Value::as_i64).unwrap_or(700);
    let files: Vec<(PathBuf, i64)> = util::arr(&Value::Object(cs.clone()), "files")
        .iter()
        .filter_map(|f| {
            let p = paths::public().join(f.get("url")?.as_str()?);
            let w = f.get("weight").and_then(|w| w.as_str().and_then(|s| s.parse().ok()).or_else(|| w.as_i64()))?;
            p.exists().then_some((p, w))
        })
        .collect();
    if let Some(best) = files.iter().min_by_key(|(_, w)| ((w - weight).abs(), *w)) {
        return best.clone();
    }
    static OLIVER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^oliver[^/]*\.(ttf|otf|woff2?)$").unwrap());
    let fonts = paths::public().join("fonts");
    match util::list_dir(&fonts).into_iter().find(|n| OLIVER.is_match(n)) {
        Some(n) => (fonts.join(n), 400),
        None => (fonts.join("PatrickHand-latin.woff2"), 700),
    }
}

/// Police chargée pour mesurer des mots à une taille donnée.
pub struct Measure {
    data: Vec<u8>,
    size: f64,
}

impl Measure {
    pub fn new(file: &Path, size: f64) -> Option<Measure> {
        let data = fonts_lib::font_bytes(file)?;
        rustybuzz::Face::from_slice(&data, 0)?;
        Some(Measure { data, size })
    }

    /// Largeur du texte en px (somme des avances après mise en forme, en 1/64 de px).
    pub fn width(&self, text: &str) -> f64 {
        let face = rustybuzz::Face::from_slice(&self.data, 0).unwrap();
        let upem = face.units_per_em() as f64;
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        let out = rustybuzz::shape(&face, &[], buf);
        let scale = self.size * 64.0 / upem;
        let total: i64 = out.glyph_positions().iter().map(|p| (p.x_advance as f64 * scale).round() as i64).sum();
        total as f64 / 64.0
    }
}

/// Le montage à rendre : edit.json, face.json et réglages des sous-titres.
pub struct Montage {
    pub edit: Value,
    pub face: Value,
    pub hd: bool,
    pub fps: f64,
    pub frames: i64,
    pub width: i64,
    pub height: i64,
    #[allow(dead_code)]
    pub landscape: bool,
    pub hab: Map<String, Value>,
    pub typed: Vec<Value>,
    pub font_file: PathBuf,
    pub font_weight: i64,
    pub cap: Map<String, Value>,
    font: Option<Measure>,
}

impl Montage {
    pub fn load(hd: bool) -> Result<Montage, String> {
        let data = paths::data();
        let edit = read_json(data.join("edit.json")).ok_or("src/data/edit.json illisible")?;
        let face = read_json(data.join("face.json")).ok_or("src/data/face.json illisible")?;
        let fps = edit.get("fps").and_then(Value::as_f64).unwrap_or(30.0);
        let frames = edit.get("durationInFrames").and_then(Value::as_i64).ok_or("durationInFrames absent d'edit.json")?;
        let (width, height) = crate::moments_lib::edit_size(Some(&edit));
        let mut hab: Map<String, Value> = match json!({"autoZoom": 1, "pop": false, "karaoke": false, "karaokeColor": "#ffd84d"}) {
            Value::Object(m) => m,
            _ => unreachable!(),
        };
        if let Some(Value::Object(h)) = edit.get("habillage") {
            for (k, v) in h {
                hab.insert(k.clone(), v.clone());
            }
        }
        let typed = edit.get("typed").and_then(Value::as_array).cloned().unwrap_or_default();
        let cs = edit.get("captionStyle").and_then(Value::as_object).cloned().unwrap_or_default();
        let (font_file, font_weight) = caption_font(&cs);
        // Sans style choisi : Oliver en gras (ou Patrick Hand si Oliver manque).
        let mut cap: Map<String, Value> = match json!({"weight": 700, "size": 64, "uppercase": true, "color": "#f4e2c2", "shadow": 50, "blur": 14}) {
            Value::Object(m) => m,
            _ => unreachable!(),
        };
        for (k, v) in &cs {
            if !v.is_null() {
                cap.insert(k.clone(), v.clone());
            }
        }
        let (ox, oy) = fonts_lib::shadow_offset(&cap);
        cap.insert("ox".into(), json!(ox));
        cap.insert("oy".into(), json!(oy));
        Ok(Montage {
            edit,
            face,
            hd,
            fps,
            frames,
            width,
            height,
            landscape: width > height,
            hab,
            typed,
            font_file,
            font_weight,
            cap,
            font: None,
        })
    }

    /// Source vidéo et clé de la position de chaque plan (rush 1080 entier, ou source 4K des
    /// images gardées).
    pub fn rush(&self) -> (PathBuf, &'static str) {
        if self.hd {
            (paths::rushes().join("rush_2160.webm"), "trimBeforeHd")
        } else {
            (paths::rushes().join("rush_1080.mp4"), "trimBefore")
        }
    }

    fn cap_size(&self) -> f64 {
        self.cap.get("size").and_then(Value::as_f64).unwrap_or(64.0)
    }

    fn uppercase(&self) -> bool {
        util::truthy(self.cap.get("uppercase"))
    }

    /// Largeur d'un mot (px CSS, taille de la police des sous-titres), majuscules appliquées.
    pub fn measure(&mut self, w: &str) -> f64 {
        if self.font.is_none() {
            self.font = Measure::new(&self.font_file, self.cap_size());
        }
        let text = if self.uppercase() { w.to_uppercase() } else { w.to_string() };
        match &self.font {
            Some(f) => f.width(&text),
            // Police illisible : largeur moyenne d'une lettre.
            None => 0.55 * self.cap_size() * text.chars().count() as f64,
        }
    }

    pub fn caption_words(&self, c: &Value) -> Vec<String> {
        if let Some(Value::Array(ws)) = c.get("words") {
            if !ws.is_empty() {
                return ws.iter().map(|w| w.as_str().map(String::from).unwrap_or_else(|| w.to_string())).collect();
            }
        }
        let parts: Vec<&str> = ["top", "bottom"].iter().filter_map(|k| c.get(*k).and_then(Value::as_str)).filter(|s| !s.is_empty()).collect();
        parts.join(" ").split(' ').map(String::from).collect()
    }

    /// Couleur de chaque mot mis en valeur sur ce moment (None sinon).
    pub fn caption_colors(&self, c: &Value, words: &[String]) -> Vec<Option<String>> {
        let start = util::fd(c, "start", 0.0);
        let hl: Vec<&Value> = util::arr(&self.edit, "highlights")
            .iter()
            .filter(|h| util::fd(h, "start", 0.0) - 0.01 <= start && start < util::fd(h, "end", 0.0))
            .collect();
        words
            .iter()
            .map(|w| {
                let mine: std::collections::HashSet<String> = w.split_whitespace().map(norm).collect();
                hl.iter()
                    .find(|h| util::s(h, "words").unwrap_or("").split_whitespace().map(norm).any(|n| !n.is_empty() && mine.contains(&n)))
                    .and_then(|h| util::s(h, "color").map(String::from))
            })
            .collect()
    }

    /// (lignes [(début, fin, taille relative)], réduction) : au plus « lines » lignes, coupées là où
    /// leurs largeurs (multipliées par la taille de chaque ligne) sont les plus proches ; réduites
    /// ensemble si la plus large dépasse. Même règle dans la page des moments (captionLayout).
    pub fn caption_layout(&mut self, words: &[String], big: &[bool]) -> (Vec<(usize, usize, f64)>, f64) {
        let space = self.measure(" ");
        let widths: Vec<f64> = words
            .iter()
            .enumerate()
            .map(|(i, w)| self.measure(w) * if big.get(i).copied().unwrap_or(false) { HIGHLIGHT_SCALE } else { 1.0 })
            .collect();
        let width = |a: usize, b: usize| widths[a..b].iter().sum::<f64>() + space * (b as f64 - a as f64 - 1.0).max(0.0);
        let lines_max = self.cap.get("lines").and_then(Value::as_i64).filter(|&l| l != 0).unwrap_or(2);
        let n = (lines_max.min(words.len() as i64)).max(1) as usize;
        let mut sizes: Vec<f64> = match self.cap.get("lineSizes").and_then(Value::as_array) {
            Some(a) if !a.is_empty() => a.iter().map(|s| s.as_f64().unwrap_or(100.0) / 100.0).collect(),
            _ => vec![1.0, 1.0, 1.0],
        };
        sizes.truncate(n);
        while sizes.len() < n {
            sizes.push(1.0);
        }
        let mut best: Option<(f64, f64)> = None;
        let mut lines = vec![(0, words.len(), sizes[0])];
        for cuts in combinations(1, words.len(), n - 1) {
            let mut bounds = vec![0];
            bounds.extend(&cuts);
            bounds.push(words.len());
            let w: Vec<f64> = bounds.windows(2).enumerate().map(|(i, ab)| width(ab[0], ab[1]) * sizes[i]).collect();
            let mx = w.iter().cloned().fold(f64::MIN, f64::max);
            let mn = w.iter().cloned().fold(f64::MAX, f64::min);
            let key = (util::round_to(mx - mn, 3), mx);
            if best.map(|b| key.0 < b.0 || (key.0 == b.0 && key.1 < b.1)).unwrap_or(true) {
                best = Some(key);
                lines = bounds.windows(2).enumerate().map(|(i, ab)| (ab[0], ab[1], sizes[i])).collect();
            }
        }
        let widest = lines.iter().map(|&(a, b, s)| width(a, b) * s).fold(f64::MIN, f64::max);
        (lines, (1.0f64).min((self.width as f64 - CAPTION_MARGIN) / widest.max(1.0)))
    }
}

/// `itertools.combinations(range(lo, hi), k)` dans le même ordre.
pub fn combinations(lo: usize, hi: usize, k: usize) -> Vec<Vec<usize>> {
    let pool: Vec<usize> = (lo..hi).collect();
    let n = pool.len();
    if k > n {
        return vec![];
    }
    let mut out = vec![];
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        out.push(idx.iter().map(|&i| pool[i]).collect());
        // Rang le plus à droite qui peut encore avancer.
        let mut i = k;
        let found = loop {
            if i == 0 {
                break None;
            }
            i -= 1;
            if idx[i] != i + n - k {
                break Some(i);
            }
        };
        let Some(i) = found else { return out };
        idx[i] += 1;
        for j in i + 1..k {
            idx[j] = idx[j - 1] + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos() {
        assert_eq!(combinations(1, 4, 1), vec![vec![1], vec![2], vec![3]]);
        assert_eq!(combinations(1, 4, 2), vec![vec![1, 2], vec![1, 3], vec![2, 3]]);
        assert_eq!(combinations(1, 4, 0), vec![Vec::<usize>::new()]);
        assert_eq!(combinations(1, 2, 2), Vec::<Vec<usize>>::new());
    }

    #[test]
    fn norm_like_page() {
        assert_eq!(norm("Économies,"), "economies");
    }
}

#[cfg(test)]
mod measure_tests {
    use super::*;
    #[test]
    fn like_pillow() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("public/fonts");
        // Oliver n'est pas versionnée (scripts de téléchargement) : vérifiée seulement si présente.
        if let Some(m) = Measure::new(&root.join("Oliver-Regular.ttf"), 64.0) {
            for (w, px) in [("BONJOUR", 254.515625), ("AVANT", 188.15625), ("Wa", 84.734375), ("TOUS,", 159.875), (" ", 16.0), ("économies", 312.515625)] {
                assert_eq!(m.width(w), px, "{w}");
            }
        }
        let m = Measure::new(&root.join("PatrickHand-latin.woff2"), 64.0).unwrap();
        for (w, px) in [("BONJOUR", 225.609375), ("AVANT", 162.4375), (" ", 14.71875)] {
            assert_eq!(m.width(w), px, "{w}");
        }
    }
}
