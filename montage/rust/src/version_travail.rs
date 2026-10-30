//! Version de travail du rush (public/rushes/rush_1080.mp4), encodée par morceaux d'une minute.
//!
//! Format du rush, petit côté à 1080 (moments_lib::work_size), H.264 à image fixe, son du rush
//! (toutes ses pistes mélangées : moments_lib::audio_map). Chaque morceau est écrit dans
//! work/reprise/travail/ (nom provisoire .part, renommé une fois complet) ; le son est extrait d'une
//! traite en FLAC (un son coupé en morceaux se décalerait de quelques centièmes de seconde à chaque
//! reprise de la lecture), puis les morceaux sont mis bout à bout et le son encodé en AAC. Une
//! préparation mise en pause, arrêtée ou coupée reprend au premier morceau manquant (mêmes rush,
//! taille et réglages : work/reprise/travail/cle.txt). Encodage par la carte graphique si elle
//! marche (gpu.rs, encodeur dans la clé), sinon par le processeur ; un échec sur la carte refait
//! les morceaux par le processeur.
//!
//! Usage : auto-montage version_travail <rush>

use crate::gpu::{self, Encoder, Rate};
use crate::moments_lib::{audio_map, display_size, work_size};
use crate::{paths, procs, progress, util};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const LABEL: &str = "Copie du rush en 1080p";
const CHUNK: i128 = 60; // secondes par morceau (arrondi à un nombre entier d'images)
const NTSC: [(&str, i128, i128); 6] = [
    ("23.98", 24000, 1001),
    ("23.976", 24000, 1001),
    ("29.97", 30000, 1001),
    ("47.95", 48000, 1001),
    ("59.94", 60000, 1001),
    ("119.88", 120000, 1001),
];

/// Fraction exacte (comme `fractions.Fraction` : toujours réduite, dénominateur positif).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frac {
    pub n: i128,
    pub d: i128,
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Division entière arrondie vers le bas (`//` de Python).
fn floor_div(a: i128, b: i128) -> i128 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

impl Frac {
    pub fn new(n: i128, d: i128) -> Frac {
        let g = gcd(n, d).max(1);
        let s = if d < 0 { -1 } else { 1 };
        Frac { n: s * n / g, d: s * d / g }
    }
    pub fn int(n: i128) -> Frac {
        Frac { n, d: 1 }
    }
    /// `Fraction("29.97")` : nombre décimal écrit en texte.
    pub fn parse(text: &str) -> Option<Frac> {
        let (ip, fp) = text.split_once('.').unwrap_or((text, ""));
        if ip.is_empty() && fp.is_empty() || fp.len() > 18 {
            return None;
        }
        let digits = format!("{ip}{fp}");
        if !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some(Frac::new(digits.parse().ok()?, 10i128.pow(fp.len() as u32)))
    }
    /// `Fraction(x)` d'un flottant : sa valeur binaire exacte.
    pub fn from_f64(x: f64) -> Frac {
        if x == 0.0 || !x.is_finite() {
            return Frac::int(0);
        }
        let bits = x.to_bits();
        let sign: i128 = if bits >> 63 == 0 { 1 } else { -1 };
        let exp = ((bits >> 52) & 0x7ff) as i32;
        let mant = if exp == 0 { (bits & 0xf_ffff_ffff_ffff) << 1 } else { (bits & 0xf_ffff_ffff_ffff) | 0x10_0000_0000_0000 } as i128;
        let e = exp - 1075;
        if e >= 0 {
            Frac::new(sign * (mant << e.min(70)), 1)
        } else if -e <= 120 {
            Frac::new(sign * mant, 1i128 << (-e))
        } else {
            Frac::int(0)
        }
    }
    /// `Fraction.limit_denominator(max)` : la fraction la plus proche de dénominateur au plus max.
    pub fn limit_denominator(self, max: i128) -> Frac {
        if self.d <= max {
            return self;
        }
        let (mut p0, mut q0, mut p1, mut q1) = (0i128, 1i128, 1i128, 0i128);
        let (mut n, mut d) = (self.n, self.d);
        loop {
            let a = floor_div(n, d);
            let q2 = q0 + a * q1;
            if q2 > max {
                break;
            }
            (p0, q0, p1, q1) = (p1, q1, p0 + a * p1, q2);
            (n, d) = (d, n - a * d);
        }
        let k = floor_div(max - q0, q1);
        if 2 * d * (q0 + k * q1) <= self.d {
            Frac::new(p1, q1)
        } else {
            Frac::new(p0 + k * p1, q0 + k * q1)
        }
    }
    pub fn f(self) -> f64 {
        self.n as f64 / self.d as f64
    }
    pub fn mul(self, o: Frac) -> Frac {
        Frac::new(self.n * o.n, self.d * o.d)
    }
    pub fn div(self, o: Frac) -> Frac {
        Frac::new(self.n * o.d, self.d * o.n)
    }
    pub fn sub(self, o: Frac) -> Frac {
        Frac::new(self.n * o.d - o.n * self.d, self.d * o.d)
    }
    /// `round(fraction)` de Python (entier, moitié au pair).
    pub fn round(self) -> i128 {
        let fl = floor_div(self.n, self.d);
        let rem2 = 2 * (self.n - fl * self.d); // 2 × partie fractionnaire × d
        if rem2 < self.d || (rem2 == self.d && fl % 2 == 0) {
            fl
        } else {
            fl + 1
        }
    }
    /// `a // b` de deux fractions.
    pub fn floor_div(self, o: Frac) -> i128 {
        floor_div(self.n * o.d, self.d * o.n)
    }
}

impl std::fmt::Display for Frac {
    /// `str(Fraction)` : « 30 » ou « 30000/1001 ».
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.d == 1 {
            write!(f, "{}", self.n)
        } else {
            write!(f, "{}/{}", self.n, self.d)
        }
    }
}

static VIDEO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Stream #\d+:\d+.*?: Video: (.*)").unwrap());

/// (durée en secondes, images par seconde) de la vidéo, lues dans « ffmpeg -i ».
fn probe(path: &Path) -> (f64, Frac) {
    let out = procs::ffmpeg_info(path);
    let duration = progress::duration_of(&out);
    let video = VIDEO.captures(&out).map(|c| c[1].to_string());
    // Cadence nominale (« tbr », celle que ffmpeg garde pour une sortie à image fixe), sinon moyenne.
    let mut fps = None;
    if let Some(v) = &video {
        for key in ["tbr", "fps"] {
            let re = Regex::new(&format!(r"([\d.]+)(k?) {key}")).unwrap();
            if let Some(r) = re.captures(v) {
                if r[2].is_empty() {
                    let f = NTSC
                        .iter()
                        .find(|(t, _, _)| *t == &r[1])
                        .map(|(_, n, d)| Frac::new(*n, *d))
                        .or_else(|| Frac::parse(&r[1]).map(|f| f.limit_denominator(1000)));
                    if let Some(f) = f {
                        if f.n >= f.d && f.n <= 120 * f.d {
                            fps = Some(f);
                            break;
                        }
                    }
                }
            }
        }
    }
    (duration, fps.unwrap_or(Frac::int(30)))
}

fn fail(msg: &str) -> i32 {
    eprintln!("{msg}");
    1
}

fn run_quiet(cmd: &[String]) -> i32 {
    procs::call(&cmd[0], &cmd[1..])
}

pub fn run(rush: &Path) -> i32 {
    let root = paths::root();
    let out = paths::rushes().join("rush_1080.mp4");
    let dir = root.join("work").join("reprise").join("travail");
    let ffmpeg = paths::display(paths::ffmpeg());
    let (duration, fps) = probe(rush);
    let (w, h) = match display_size(rush) {
        Some((a, b)) => work_size(a, b, 1080),
        None => (1080, 1920),
    };
    let Some(amap) = audio_map(rush, "son") else {
        return fail("Cette vidéo n'a pas de son.");
    };
    let frames = Frac::int(CHUNK).mul(fps).round().max(1); // morceaux d'un nombre entier d'images
    let step = Frac::int(frames).div(fps);
    // Dernier morceau : jusqu'à la fin (pas de morceau de moins d'une seconde).
    let rest = Frac::from_f64(duration).limit_denominator(1000).sub(Frac::int(1));
    let count = (-Frac::new(-rest.n, rest.d).floor_div(step)).max(1) as usize;
    let meta = fs::metadata(rush);
    let (size, mtime) = match &meta {
        Ok(m) => (m.len(), util::mtime(rush).unwrap_or(0.0) as i64),
        Err(_) => (0, 0),
    };
    let name = rush.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    // Encodeur : carte graphique si elle marche (gpu.rs), sinon processeur ; tous les morceaux du
    // même (clé de reprise), et un échec sur la carte refait tous les morceaux par le processeur.
    let mut enc = gpu::h264();
    let chunk_path = |i: usize| -> PathBuf { dir.join(format!("{i:04}.mkv")) };
    let sound = dir.join("son.flac");
    let rush_s = paths::display(rush);
    let stepf = step.f();
    loop {
        let key = format!("{name} {size} {mtime} {w}x{h} {fps} {frames} 2{}\n", if enc.is_gpu() { format!(" {}", enc.name) } else { String::new() });
        util::mkdirs(&dir);
        let key_file = dir.join("cle.txt");
        if fs::read_to_string(&key_file).ok().as_deref() != Some(key.as_str()) {
            util::remove_all(&dir);
            util::mkdirs(&dir);
            let _ = fs::write(&key_file, &key);
        }
        let done = (0..count).filter(|i| chunk_path(*i).exists()).count();
        if done > 0 {
            progress::say(&format!("Reprise de la copie du rush : {done} morceau(x) sur {count} déjà faits"));
        }
        progress::say(&format!("Copie du rush {w}x{h}, {:.3} images/s, {count} morceau(x)", fps.f()));
        if !sound.exists() {
            let part = dir.join("son.part.flac");
            let mut cmd = vec![ffmpeg.clone(), "-v".into(), "error".into(), "-y".into(), "-i".into(), rush_s.clone()];
            cmd.extend(amap.iter().cloned());
            cmd.extend(["-c:a".into(), "flac".into(), paths::display(&part)]);
            if run_quiet(&cmd) != 0 {
                return fail("Échec de l'extraction du son du rush");
            }
            if fs::rename(&part, &sound).is_err() {
                return fail("Échec de l'extraction du son du rush");
            }
        }
        let mut failed = None;
        for i in 0..count {
            let chunk = chunk_path(i);
            let start = Frac::int(i as i128).mul(step).f();
            let length = if i < count - 1 { stepf } else { (duration - start + 1.0).max(0.1) };
            if chunk.exists() {
                continue;
            }
            let part = dir.join(format!("{i:04}.part.mkv"));
            let mut cmd: Vec<String> = vec![ffmpeg.clone(), "-v".into(), "error".into(), "-y".into(), "-ss".into(), format!("{start:.6}")];
            cmd.extend(enc.input_args());
            cmd.extend(["-i".into(), rush_s.clone(), "-t".into(), format!("{length:.6}"), "-map".into(), "0:v:0".into(), "-an".into()]);
            cmd.extend(["-vf".into(), format!("scale={w}:{h},setsar=1{}", enc.filter_tail()), "-fps_mode".into(), "cfr".into(), "-r".into(), fps.to_string()]);
            cmd.extend(enc.video_args(Rate::Crf(18), "veryfast"));
            cmd.extend(["-g".into(), "15".into(), paths::display(&part)]);
            let code = progress::ffmpeg(LABEL, &cmd, Some(length.min((duration - start).max(0.1))), start, Some(duration));
            if code != 0 || fs::rename(&part, &chunk).is_err() {
                failed = Some(i);
                break;
            }
        }
        match failed {
            None => break,
            Some(_) if enc.is_gpu() => {
                gpu::give_up(&enc);
                enc = Encoder::cpu();
            }
            Some(i) => return fail(&format!("Échec de la copie du rush (morceau {} sur {count})", i + 1)),
        }
    }
    // Morceaux bout à bout (chacun dure exactement « step » secondes), son encodé en AAC.
    let listing = dir.join("liste.txt");
    let mut text = String::new();
    for i in 0..count {
        text += &format!("file '{i:04}.mkv'\n");
        if i < count - 1 {
            text += &format!("duration {stepf:.6}\n");
        }
    }
    if fs::write(&listing, text).is_err() {
        return fail("Échec de l'assemblage de la copie du rush");
    }
    let part = paths::rushes().join("rush_1080.part.mp4");
    let cmd: Vec<String> = [
        ffmpeg.as_str(),
        "-v",
        "error",
        "-y",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        &paths::display(&listing),
        "-i",
        &paths::display(&sound),
        "-map",
        "0:v:0",
        "-map",
        "1:a:0",
        "-c:v",
        "copy",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        &paths::display(&part),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if run_quiet(&cmd) != 0 || fs::rename(&part, &out).is_err() {
        return fail("Échec de l'assemblage de la copie du rush");
    }
    util::remove_all(&dir);
    progress::report_every(LABEL, duration, duration, 0.0);
    let rel = out.strip_prefix(root).map(Path::to_path_buf).unwrap_or(out.clone());
    progress::say(&format!("Copie du rush prête : {}", paths::display(&rel)));
    0
}

pub fn main(args: &[String]) -> i32 {
    if args.len() != 1 {
        eprintln!("Version de travail du rush (public/rushes/rush_1080.mp4), encodée par morceaux d'une minute.\n\nUsage : auto-montage version_travail <rush>");
        return 1;
    }
    run(Path::new(&args[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractions_like_python() {
        assert_eq!(Frac::parse("29.97").unwrap().limit_denominator(1000).to_string(), "2997/100");
        assert_eq!(Frac::parse("25").unwrap().to_string(), "25");
        // Fraction(26.04).limit_denominator(1000) == Fraction(651, 25)
        assert_eq!(Frac::from_f64(26.04).limit_denominator(1000), Frac::new(651, 25));
        assert_eq!(Frac::from_f64(0.1).limit_denominator(1000), Frac::new(1, 10));
        // round(60 * 30000/1001) == 1798
        assert_eq!(Frac::int(60).mul(Frac::new(30000, 1001)).round(), 1798);
        assert_eq!(Frac::new(5, 2).round(), 2);
        assert_eq!(Frac::new(7, 2).round(), 4);
        // Fraction(1, 3).limit_denominator(10) cas d'un décimal infini
        assert_eq!(Frac::from_f64(1.0 / 3.0).limit_denominator(1000), Frac::new(1, 3));
        assert_eq!(Frac::from_f64(3.14159265).limit_denominator(1000), Frac::new(355, 113));
    }
}
