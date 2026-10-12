//! Analyse des vidéos d'exemple (work/style/videos/), première étape : ce qui se mesure.
//!
//! Pour chaque vidéo :
//!   - les coupes (changements de plan, filtre « scene » de ffmpeg) : rythme du montage ;
//!   - des images datées : régulièrement et juste après chaque coupe (planches de 4 x 3 images
//!     avec le temps écrit dessus), que Claude regarde ensuite (auto-montage style_claude) ;
//!   - les attaques sonores (montées brusques du volume hors parole continue) : bruitages
//!     probables, avec leur instant ;
//!   - la taille et la durée.
//! Sorties : work/style/analyse/faits.json et work/style/analyse/<vidéo>/planche-<n>.jpg.
//! Avancement : lignes « @progression » (progress).
//!
//! Images : décodées et assemblées avec le crate image (redimensionnement bilinéaire, comme
//! cv2.resize) ; le temps est écrit dans une police du projet (Poppins), rendue ici.

use crate::paths;
use crate::progress::{report, say};
use crate::pyjson;
use crate::util::{self, round_to};
use image::{imageops, Rgb, RgbImage};
use regex::Regex;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

const VIDEO_EXT: [&str; 7] = [".mov", ".mp4", ".m4v", ".mkv", ".webm", ".avi", ".mts"];
const LABEL: &str = "Analyse des vidéos d'exemple";
const FRAMES_MAX: usize = 36; // images par vidéo (3 planches de 12)

fn videos_dir() -> PathBuf {
    paths::work().join("style").join("videos")
}
fn out_dir() -> PathBuf {
    paths::work().join("style").join("analyse")
}

struct Info {
    duration: f64,
    width: i64,
    height: i64,
}

static DURATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Duration: (\d+):(\d+):([\d.]+)").unwrap());
static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Video: .*?, (\d{2,5})x(\d{2,5})").unwrap());
static ROT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"rotation of (-?[\d.]+) degrees").unwrap());
static PTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"pts_time:([\d.]+)").unwrap());

fn info(path: &Path) -> Info {
    let out = crate::procs::ffmpeg_info(path);
    let (mut w, mut h) = SIZE
        .captures(&out)
        .map(|c| (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0)))
        .unwrap_or((0, 0));
    if let Some(r) = ROT.captures(&out) {
        if util::round_int(r[1].parse::<f64>().unwrap_or(0.0).abs()) % 180 == 90 {
            std::mem::swap(&mut w, &mut h);
        }
    }
    let duration = DURATION
        .captures(&out)
        .map(|c| c[1].parse::<f64>().unwrap_or(0.0) * 3600.0 + c[2].parse::<f64>().unwrap_or(0.0) * 60.0 + c[3].parse::<f64>().unwrap_or(0.0))
        .unwrap_or(0.0);
    Info { duration, width: w, height: h }
}

/// Instants des changements de plan (score de scène > 0,3), avec l'avancement.
fn scene_cuts(path: &Path, duration: f64, done_before: f64, total: f64) -> Vec<f64> {
    let mut child = match Command::new(paths::ffmpeg())
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(path)
        .args(["-an", "-vf", "scale=320:-2,select='gt(scene,0.3)',showinfo", "-f", "null", "-", "-progress", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let err = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut cuts = vec![];
        for line in BufReader::new(err).split(b'\n').map_while(Result::ok) {
            let line = String::from_utf8_lossy(&line);
            if let Some(c) = PTS.captures(&line) {
                cuts.push(round_to(c[1].parse::<f64>().unwrap_or(0.0), 2));
            }
        }
        cuts
    });
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        let (key, value) = line.trim().split_once('=').unwrap_or((line.trim(), ""));
        if key == "out_time_us" && !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) {
            let t = value.parse::<f64>().unwrap_or(0.0) / 1e6;
            report(LABEL, done_before + 0.7 * duration.min(t), total);
        }
    }
    let _ = child.wait();
    let mut cuts = reader.join().unwrap_or_default();
    sort_dedup(&mut cuts);
    cuts
}

fn sort_dedup(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v.dedup();
}

/// Images datées : régulières (environ 2 par seconde de plan) et juste après chaque coupe.
fn frame_times(duration: f64, cuts: &[f64]) -> Vec<f64> {
    let regular: Vec<f64> = if duration > 0.0 { (0..18).map(|i| duration * (i as f64 + 0.5) / 18.0).collect() } else { vec![0.0] };
    let after: Vec<f64> = cuts.iter().map(|c| (duration - 0.05).min(c + 0.15)).collect();
    let mut times: Vec<f64> = regular
        .iter()
        .chain(after.iter())
        .filter(|t| 0.0 <= **t && **t < duration.max(0.1))
        .map(|t| round_to(*t, 2))
        .collect();
    sort_dedup(&mut times);
    while times.len() > FRAMES_MAX {
        // trop d'images : on en garde une sur deux, coupes d'abord
        let mut next: Vec<f64> = after.iter().take(FRAMES_MAX / 2).map(|t| round_to(*t, 2)).collect();
        next.extend(times.iter().step_by(2));
        sort_dedup(&mut next);
        next.truncate(FRAMES_MAX);
        times = next;
    }
    times
}

fn grab(path: &Path, t: f64, width: u32) -> Option<RgbImage> {
    let out = Command::new(paths::ffmpeg())
        .args(["-v", "error", "-ss", &format!("{t:.2}"), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &format!("scale={width}:-2"), "-f", "image2pipe", "-vcodec", "png", "-"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    image::load_from_memory(&out.stdout).ok().map(|i| i.to_rgb8())
}

/// Texte blanc sur un bandeau noir en haut de l'image (comme cv2.rectangle + cv2.putText).
fn label(img: &mut RgbImage, text: &str) {
    let w = img.width();
    for y in 0..27.min(img.height()) {
        for x in 0..w {
            img.put_pixel(x, y, Rgb([0, 0, 0]));
        }
    }
    draw_text(img, text, 6.0, 19.0, 18.0);
}

/// Contour d'un glyphe aplati en segments (pixels de l'image, y vers le bas).
struct Outline {
    segs: Vec<((f64, f64), (f64, f64))>,
    start: (f64, f64),
    cur: (f64, f64),
    scale: f64,
    ox: f64,
    oy: f64,
}

impl Outline {
    fn pt(&self, x: f32, y: f32) -> (f64, f64) {
        (self.ox + x as f64 * self.scale, self.oy - y as f64 * self.scale)
    }
    fn line(&mut self, p: (f64, f64)) {
        self.segs.push((self.cur, p));
        self.cur = p;
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.cur = self.pt(x, y);
        self.start = self.cur;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.pt(x, y);
        self.line(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.cur, self.pt(x1, y1), self.pt(x, y));
        for i in 1..=8 {
            let t = i as f64 / 8.0;
            let u = 1.0 - t;
            self.line((u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0, u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, p1, p2, p3) = (self.cur, self.pt(x1, y1), self.pt(x2, y2), self.pt(x, y));
        for i in 1..=10 {
            let t = i as f64 / 10.0;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.line((a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1));
        }
    }
    fn close(&mut self) {
        let s = self.start;
        self.line(s);
    }
}

static FONT: LazyLock<Option<Vec<u8>>> = LazyLock::new(|| crate::fonts_lib::font_bytes(&crate::render_lib::poppins()));

/// Écrit le texte (ligne de base en y, px de hauteur d'em) : glyphes remplis (règle non nulle),
/// lissés par 4 x 4 échantillons par pixel.
fn draw_text(img: &mut RgbImage, text: &str, x: f64, y: f64, px: f64) {
    let Some(data) = FONT.as_ref() else { return };
    let Ok(face) = ttf_parser::Face::parse(data, 0) else { return };
    let scale = px / face.units_per_em() as f64;
    let mut o = Outline { segs: vec![], start: (0.0, 0.0), cur: (0.0, 0.0), scale, ox: x, oy: y };
    for ch in text.chars() {
        let Some(g) = face.glyph_index(ch) else {
            o.ox += px * 0.3;
            continue;
        };
        face.outline_glyph(g, &mut o);
        o.ox += face.glyph_hor_advance(g).unwrap_or(0) as f64 * scale;
    }
    if o.segs.is_empty() {
        return;
    }
    let (x0, x1) = o.segs.iter().fold((f64::MAX, f64::MIN), |(a, b), (p, q)| (a.min(p.0).min(q.0), b.max(p.0).max(q.0)));
    let (y0, y1) = o.segs.iter().fold((f64::MAX, f64::MIN), |(a, b), (p, q)| (a.min(p.1).min(q.1), b.max(p.1).max(q.1)));
    let (w, h) = (img.width() as i64, img.height() as i64);
    const N: usize = 4;
    for py in (y0.floor() as i64).max(0)..=(y1.ceil() as i64).min(h - 1) {
        // Croisements de chaque ligne d'échantillons avec le contour.
        let mut rows: Vec<Vec<(f64, i32)>> = vec![];
        for sy in 0..N {
            let yy = py as f64 + (sy as f64 + 0.5) / N as f64;
            let mut xs: Vec<(f64, i32)> = o
                .segs
                .iter()
                .filter_map(|&((ax, ay), (bx, by))| {
                    if (ay <= yy && by > yy) || (by <= yy && ay > yy) {
                        let t = (yy - ay) / (by - ay);
                        Some((ax + t * (bx - ax), if by > ay { 1 } else { -1 }))
                    } else {
                        None
                    }
                })
                .collect();
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            rows.push(xs);
        }
        for px_ in (x0.floor() as i64).max(0)..=(x1.ceil() as i64).min(w - 1) {
            let mut inside = 0;
            for xs in &rows {
                for sx in 0..N {
                    let xx = px_ as f64 + (sx as f64 + 0.5) / N as f64;
                    let wind: i32 = xs.iter().take_while(|(x, _)| *x < xx).map(|(_, d)| d).sum();
                    if wind != 0 {
                        inside += 1;
                    }
                }
            }
            if inside > 0 {
                let a = inside as f64 / (N * N) as f64;
                let p = img.get_pixel_mut(px_ as u32, py as u32);
                for c in 0..3 {
                    p.0[c] = (p.0[c] as f64 * (1.0 - a) + 255.0 * a).round() as u8;
                }
            }
        }
    }
}

/// Planches de 4 x 3 images datées.
fn sheets(frames: &[(String, RgbImage)], folder: &Path) -> Vec<String> {
    let mut paths_ = vec![];
    for (n, chunk) in frames.chunks(12).enumerate() {
        let h = chunk.iter().map(|(_, f)| f.height()).max().unwrap_or(1);
        let w = chunk[0].1.width();
        let mut tiles: Vec<RgbImage> = chunk
            .iter()
            .map(|(text, f)| {
                let mut t = if f.width() == w && f.height() == h { f.clone() } else { imageops::resize(f, w, h, imageops::FilterType::Triangle) };
                label(&mut t, text);
                t
            })
            .collect();
        while tiles.len() % 4 != 0 {
            tiles.push(RgbImage::new(w, h));
        }
        let rows = tiles.len() / 4;
        let mut sheet = RgbImage::new(w * 4, h * rows as u32);
        for (i, t) in tiles.iter().enumerate() {
            imageops::replace(&mut sheet, t, (i % 4) as i64 * w as i64, (i / 4) as i64 * h as i64);
        }
        let path = folder.join(format!("planche-{}.jpg", n + 1));
        if let Ok(file) = std::fs::File::create(&path) {
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 82);
            if let Err(e) = enc.encode_image(&sheet) {
                say(&format!("{} : {e}", path.display()));
            }
        }
        let rel = path.strip_prefix(paths::work()).map(Path::to_path_buf).unwrap_or(path.clone());
        paths_.push(rel.to_string_lossy().into_owned());
    }
    paths_
}

/// Attaques sonores nettes (bruitages probables) : énergie sur 50 ms qui bondit au-dessus
/// du niveau des 0,5 s précédentes.
fn sound_onsets(path: &Path) -> Vec<f64> {
    let Ok(mut child) = Command::new(paths::ffmpeg())
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "8000", "-f", "s16le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
    else {
        return vec![];
    };
    let mut raw = vec![];
    let _ = child.stdout.take().unwrap().read_to_end(&mut raw);
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    if !ok || raw.is_empty() {
        return vec![];
    }
    let x: Vec<f64> = raw.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64 / 32768.0).collect();
    let hop = 400; // 50 ms
    let n = x.len() / hop;
    if n < 20 {
        return vec![];
    }
    let e: Vec<f64> = (0..n).map(|i| (x[i * hop..(i + 1) * hop].iter().map(|v| v * v).sum::<f64>() / hop as f64).sqrt() + 1e-4).collect();
    let (mut out, mut last) = (vec![], -1.0);
    for i in 10..n {
        let mut win = e[i - 10..i].to_vec();
        win.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let before = (win[4] + win[5]) / 2.0; // médiane de 10 valeurs
        let t = (i * hop) as f64 / 8000.0;
        if e[i] > 4.0 * before && e[i] > 0.05 && t - last > 0.4 {
            out.push(round_to(t, 2));
            last = t;
        }
    }
    out.truncate(60);
    out
}

static FOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w.-]+").unwrap());

pub fn main(_args: &[String]) -> i32 {
    let vids = videos_dir();
    let names: Vec<String> = util::list_dir(&vids)
        .into_iter()
        .filter(|n| {
            let l = n.to_lowercase();
            VIDEO_EXT.iter().any(|e| l.ends_with(e)) && !n.starts_with('.')
        })
        .collect();
    if names.is_empty() {
        eprintln!("Aucune vidéo d'exemple (work/style/videos/).");
        return 1;
    }
    let out = out_dir();
    let _ = std::fs::remove_dir_all(&out);
    util::mkdirs(&out);
    let infos: Vec<Info> = names.iter().map(|n| info(&vids.join(n))).collect();
    let total: f64 = infos.iter().map(|i| i.duration.max(1.0)).sum();
    let mut done = 0.0;
    report(LABEL, 0.0, total);
    let mut facts = vec![];
    for (name, meta) in names.iter().zip(&infos) {
        let path = vids.join(name);
        let dur = meta.duration.max(1.0);
        say(&format!("{name} : {:.1} s, {}x{}", meta.duration, meta.width, meta.height));
        let cuts = scene_cuts(&path, dur, done, total);
        let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let folder = out.join(FOLDER.replace_all(&stem, "_").as_ref());
        util::mkdirs(&folder);
        let mut frames = vec![];
        let times = frame_times(meta.duration, &cuts);
        for (k, &t) in times.iter().enumerate() {
            if let Some(img) = grab(&path, t, 300) {
                let cut = cuts.iter().any(|c| (t - c - 0.15).abs() < 0.02);
                frames.push((format!("{t:.1} s{}", if cut { " (coupe)" } else { "" }), img));
            }
            report(LABEL, done + dur * (0.7 + 0.25 * (k + 1) as f64 / times.len() as f64), total);
        }
        let onsets = sound_onsets(&path);
        let shots = cuts.len() + 1;
        let sheet_paths = if frames.is_empty() { vec![] } else { sheets(&frames, &folder) };
        facts.push(json!({
            "video": name, "duration": util::num(round_to(meta.duration, 1)), "size": format!("{}x{}", meta.width, meta.height),
            "cuts": cuts.iter().map(|c| util::num(*c)).collect::<Vec<_>>(), "shots": shots,
            "averageShot": util::num(round_to(meta.duration / shots as f64, 2)),
            "cutsPerMinute": util::num(round_to(cuts.len() as f64 * 60.0 / dur, 1)),
            "soundOnsets": onsets.iter().map(|c| util::num(*c)).collect::<Vec<_>>(),
            "sheets": sheet_paths,
        }));
        say(&format!(
            "  {} coupes (un plan toutes les {:.1} s), {} attaques sonores, {} images",
            cuts.len(),
            meta.duration / shots as f64,
            onsets.len(),
            frames.len()
        ));
        done += dur;
        report(LABEL, done, total);
    }
    let n = facts.len();
    let text = pyjson::dumps_with(&json!({"videos": Value::Array(facts)}), pyjson::Opts { indent: Some(1), ensure_ascii: false, sort_keys: false });
    if let Err(e) = std::fs::write(out.join("faits.json"), text) {
        eprintln!("work/style/analyse/faits.json : {e}");
        return 1;
    }
    say(&format!("Faits mesurés : work/style/analyse/faits.json ({n} vidéo{})", if n > 1 { "s" } else { "" }));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_like_python() {
        // Valeurs de la version Python : frame_times(10, [2.5, 6.0]).
        let t = frame_times(10.0, &[2.5, 6.0]);
        assert_eq!(t.len(), 20);
        assert_eq!(t[0], 0.28);
        assert!(t.contains(&2.65) && t.contains(&6.15));
        assert_eq!(frame_times(0.0, &[]), vec![0.0]);
        // Beaucoup de coupes : 36 images au plus.
        let cuts: Vec<f64> = (0..80).map(|i| i as f64 * 0.7).collect();
        assert_eq!(frame_times(60.0, &cuts).len(), FRAMES_MAX);
    }
}
