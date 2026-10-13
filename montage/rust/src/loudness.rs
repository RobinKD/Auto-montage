//! Volume perçu (sonie) d'après l'UIT-R BS.1770-4 / EBU R128, et normalisation du montage.
//!
//! Mesure reprise du compteur de FilmCraft (crates/audio-dsp : loudness.rs, biquad.rs,
//! oversample.rs ; https://github.com/storytold/filmcraft, licence MIT ou Apache-2.0,
//! © 2026 ArtCraft Team et contributeurs de FilmCraft), ramenée à ce dont Auto-montage a besoin :
//!   - pondération K (pré-filtre en plateau puis passe-haut RLB, coefficients tirés des
//!     prototypes analogiques : n'importe quelle fréquence d'échantillonnage) ;
//!   - sonie intégrée avec les deux portes (−70 LUFS absolue, −10 LU relative), blocs de 400 ms
//!     tous les 100 ms, histogramme au 1/100 de LU qui garde les énergies exactes ;
//!   - plage de sonie (EBU Tech 3342) et crête vraie (suréchantillonnage polyphase ×4 à 48 kHz).
//!
//! Normalisation (réglage « loudness » de work/son.json, en LUFS, −14 par défaut ; false =
//! désactivée) :
//!   - build_edit : la piste voix est amenée près de la cible (gain, puis limiteur à anticipation
//!     qui garde la crête vraie sous −1,5 dBTP) ; le mixage final se fait au rendu ;
//!   - render (après HyperFrames) : le son de la vidéo rendue est mesuré puis corrigé pour tomber
//!     sur la cible à ±0,5 LU, crête vraie sous −1 dBTP (image copiée telle quelle), résultat
//!     dans work/volume.json ;
//!   - préréglages d'export (export.rs) : chaque plateforme a sa cible.
//!
//! Commande : auto-montage loudness <fichier> [--cible LUFS [--sortie <fichier>]]
//!   mesure (et normalise vers --sortie, image copiée).

use crate::paths;
use crate::progress::say;
use crate::util;
use serde_json::{json, Map, Value};
use std::f64::consts::PI;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

/// Cible par défaut : celle des plateformes (YouTube, TikTok, Instagram, Spotify).
pub const DEFAULT_TARGET: f64 = -14.0;
/// Crête vraie maximale du fichier rendu (marge pour l'encodage AAC).
pub const MAX_TRUE_PEAK: f64 = -1.0;
const SR: f64 = 48000.0;

// --- Filtres du second ordre (forme directe II transposée) ----------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    z1: f64,
    z2: f64,
}

impl Biquad {
    #[inline(always)]
    fn tick(&mut self, c: &Coeffs, x: f64) -> f64 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}

/// Premier étage de la pondération K : plateau haut (« tête »).
pub fn k_prefilter(sample_rate: f64) -> Coeffs {
    let f0 = 1_681.974_450_955_533;
    let gain_db = 3.999_843_853_973_347;
    let q = 0.707_175_236_955_419_6;
    let k = (PI * f0 / sample_rate).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    Coeffs {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

/// Second étage : passe-haut RLB.
pub fn k_rlb(sample_rate: f64) -> Coeffs {
    let f0 = 38.135_470_876_024_44;
    let q = 0.500_327_037_323_877_3;
    let k = (PI * f0 / sample_rate).tan();
    let a0 = 1.0 + k / q + k * k;
    Coeffs { b0: 1.0, b1: -2.0, b2: 1.0, a1: 2.0 * (k * k - 1.0) / a0, a2: (1.0 - k / q + k * k) / a0 }
}

/// Sonie d'une énergie quadratique moyenne (déjà pondérée par canal), en LUFS.
pub fn energy_to_lufs(e: f64) -> f64 {
    if e <= 0.0 {
        f64::NEG_INFINITY
    } else {
        -0.691 + 10.0 * e.log10()
    }
}

// --- Crête vraie : interpolateur polyphase (sinus cardinal fenêtré de Kaiser) ---------------

const TAPS: usize = 16;

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0, 1.0, x * x / 4.0);
    for k in 1..50 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

#[derive(Clone, Debug)]
struct Polyphase {
    factor: usize,
    coeffs: Vec<f32>,
}

impl Polyphase {
    /// ×4 sous 88,2 kHz, ×2 sous 176,4 kHz (au moins 176,4 kHz effectifs, BS.1770-4).
    fn new(sample_rate: f64) -> Self {
        let factor = if sample_rate < 88_200.0 { 4 } else if sample_rate < 176_400.0 { 2 } else { 1 };
        let len = factor * TAPS;
        let centre = (len - 1) as f64 / 2.0;
        let (beta, i0b) = (8.0, bessel_i0(8.0));
        let proto: Vec<f64> = (0..len)
            .map(|m| {
                let t = (m as f64 - centre) / factor as f64;
                let sinc = if t.abs() < 1e-12 { 1.0 } else { (PI * t).sin() / (PI * t) };
                let r = (m as f64 - centre) / (centre + 1.0);
                sinc * bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0b
            })
            .collect();
        let mut coeffs = vec![0f32; len];
        for p in 0..factor {
            let sum: f64 = (0..TAPS).map(|j| proto[p + factor * j]).sum();
            for j in 0..TAPS {
                coeffs[p * TAPS + j] = (proto[p + factor * j] / sum) as f32;
            }
        }
        Polyphase { factor, coeffs }
    }
}

#[derive(Clone, Debug)]
struct Interpolator {
    hist: [f32; 2 * TAPS],
    pos: usize,
}

impl Default for Interpolator {
    fn default() -> Self {
        Interpolator { hist: [0.0; 2 * TAPS], pos: 0 }
    }
}

impl Interpolator {
    /// Ajoute un échantillon ; renvoie la plus grande valeur absolue des échantillons interpolés.
    #[inline]
    fn push_peak(&mut self, bank: &Polyphase, x: f32) -> f32 {
        if bank.factor == 1 {
            return x.abs();
        }
        self.pos = if self.pos == 0 { TAPS - 1 } else { self.pos - 1 };
        self.hist[self.pos] = x;
        self.hist[self.pos + TAPS] = x;
        let win = &self.hist[self.pos..self.pos + TAPS];
        let mut peak = 0f32;
        for p in 0..bank.factor {
            let c = &bank.coeffs[p * TAPS..(p + 1) * TAPS];
            let y: f32 = c.iter().zip(win).map(|(a, b)| a * b).sum();
            peak = peak.max(y.abs());
        }
        peak
    }
}

// --- Compteur ---------------------------------------------------------------------------------

const HIST_MIN: f64 = -70.0;
const HIST_STEP: f64 = 0.01;
const HIST_BINS: usize = 9000; // −70 à +20 LUFS
const ST_SUB: usize = 30; // fenêtre courte (3 s) en sous-blocs de 100 ms
const M_SUB: usize = 4; // fenêtre momentanée (400 ms)

#[derive(Clone, Debug)]
struct Histogram {
    count: Vec<u64>,
    energy: Vec<f64>,
}

impl Histogram {
    fn new() -> Self {
        Histogram { count: vec![0; HIST_BINS], energy: vec![0.0; HIST_BINS] }
    }
    fn bin_of(l: f64) -> usize {
        (((l - HIST_MIN) / HIST_STEP).floor().max(0.0) as usize).min(HIST_BINS - 1)
    }
    fn centre(i: usize) -> f64 {
        HIST_MIN + (i as f64 + 0.5) * HIST_STEP
    }
    fn add(&mut self, energy: f64) {
        let l = energy_to_lufs(energy);
        if l >= HIST_MIN {
            let b = Self::bin_of(l);
            self.count[b] += 1;
            self.energy[b] += energy;
        }
    }
    fn mean_from(&self, from: usize) -> Option<f64> {
        let (mut e, mut n) = (0.0, 0u64);
        for i in from..HIST_BINS {
            e += self.energy[i];
            n += self.count[i];
        }
        (n > 0).then(|| e / n as f64)
    }
    fn gate_bin(&self, rel: f64) -> Option<usize> {
        let thr = energy_to_lufs(self.mean_from(0)?) + rel;
        Some(if thr < HIST_MIN { 0 } else { Self::bin_of(thr) })
    }
}

/// Compteur BS.1770-4 / EBU R128 pour un son entrelacé (1 ou 2 canaux, poids 1).
#[derive(Clone, Debug)]
pub struct Meter {
    channels: usize,
    pre: Coeffs,
    rlb: Coeffs,
    filt: Vec<(Biquad, Biquad)>,
    acc: Vec<f64>,
    sub_len: usize,
    sub_pos: usize,
    ring: [f64; ST_SUB],
    ring_pos: usize,
    seen: u64,
    integrated: Histogram,
    lra: Histogram,
    true_peak: f32,
    bank: Polyphase,
    interp: Vec<Interpolator>,
}

/// Mesures d'un programme entier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    /// Sonie intégrée (LUFS) ; −∞ pour un silence.
    pub integrated: f64,
    /// Plage de sonie (LU).
    pub range: f64,
    /// Crête vraie (dBTP).
    pub true_peak: f64,
}

impl Meter {
    pub fn new(sample_rate: f64, channels: usize) -> Self {
        let channels = channels.max(1);
        Meter {
            channels,
            pre: k_prefilter(sample_rate),
            rlb: k_rlb(sample_rate),
            filt: vec![(Biquad::default(), Biquad::default()); channels],
            acc: vec![0.0; channels],
            sub_len: ((sample_rate * 0.1).round() as usize).max(1),
            sub_pos: 0,
            ring: [0.0; ST_SUB],
            ring_pos: 0,
            seen: 0,
            integrated: Histogram::new(),
            lra: Histogram::new(),
            true_peak: 0.0,
            bank: Polyphase::new(sample_rate),
            interp: vec![Interpolator::default(); channels],
        }
    }

    /// Son entrelacé (`channels` valeurs par instant).
    pub fn process(&mut self, data: &[f32]) {
        let nch = self.channels;
        for frame in data.chunks_exact(nch) {
            for (ch, &x) in frame.iter().enumerate() {
                let (pre, rlb) = &mut self.filt[ch];
                let y = rlb.tick(&self.rlb, pre.tick(&self.pre, x as f64));
                self.acc[ch] += y * y;
                let tp = self.interp[ch].push_peak(&self.bank, x).max(x.abs());
                if tp > self.true_peak {
                    self.true_peak = tp;
                }
            }
            self.sub_pos += 1;
            if self.sub_pos == self.sub_len {
                self.finish_subblock();
            }
        }
    }

    fn finish_subblock(&mut self) {
        let e: f64 = self.acc.iter().sum();
        self.acc.iter_mut().for_each(|a| *a = 0.0);
        self.ring[self.ring_pos] = e;
        self.ring_pos = (self.ring_pos + 1) % ST_SUB;
        self.sub_pos = 0;
        self.seen += 1;
        let window = |k: usize| (0..k).map(|i| self.ring[(self.ring_pos + ST_SUB - 1 - i) % ST_SUB]).sum::<f64>() / (k * self.sub_len) as f64;
        let (em, es) = (window(M_SUB), window(ST_SUB));
        if self.seen >= M_SUB as u64 {
            self.integrated.add(em);
        }
        if self.seen >= ST_SUB as u64 {
            self.lra.add(es);
        }
    }

    /// Sonie intégrée (porte relative à −10 LU).
    pub fn integrated(&self) -> f64 {
        let h = &self.integrated;
        match h.gate_bin(-10.0).and_then(|b| h.mean_from(b)) {
            Some(e) => energy_to_lufs(e),
            None => f64::NEG_INFINITY,
        }
    }

    /// Plage de sonie (EBU Tech 3342) : 95e − 10e centile des sonies courtes, porte à −20 LU.
    pub fn range(&self) -> f64 {
        let h = &self.lra;
        let Some(start) = h.gate_bin(-20.0) else { return 0.0 };
        let n: u64 = h.count[start..].iter().sum();
        if n == 0 {
            return 0.0;
        }
        let pct = |p: f64| {
            let rank = ((p * (n - 1) as f64).round() as u64).min(n - 1);
            let mut c = 0u64;
            for i in start..HIST_BINS {
                c += h.count[i];
                if c > rank {
                    return Histogram::centre(i);
                }
            }
            Histogram::centre(HIST_BINS - 1)
        };
        (pct(0.95) - pct(0.10)).max(0.0)
    }

    pub fn summary(&self) -> Summary {
        Summary { integrated: self.integrated(), range: self.range(), true_peak: 20.0 * (self.true_peak as f64).log10() }
    }
}

/// Mesure d'un son entrelacé entier.
pub fn measure(data: &[f32], sample_rate: f64, channels: usize) -> Summary {
    let mut m = Meter::new(sample_rate, channels);
    m.process(data);
    m.summary()
}

// --- Limiteur à anticipation -------------------------------------------------------------------

/// Limiteur à anticipation (5 ms) qui garde la crête vraie du son entrelacé sous `ceiling_db` :
/// gain calculé sur la crête vraie (interpolée ×4) de chaque instant, réduit d'avance sur la
/// fenêtre d'anticipation, relâché en 80 ms. Une seule passe, en place.
pub fn limit(data: &mut [f32], sample_rate: f64, channels: usize, ceiling_db: f64) {
    let nch = channels.max(1);
    let frames = data.len() / nch;
    if frames == 0 {
        return;
    }
    let ceiling = 10f64.powf(ceiling_db / 20.0) as f32;
    let look = ((sample_rate * 0.005) as usize).max(1);
    // Gain voulu à chaque instant (crête vraie de tous les canaux).
    let bank = Polyphase::new(sample_rate);
    let mut interp = vec![Interpolator::default(); nch];
    let mut want = vec![1f32; frames];
    for f in 0..frames {
        let mut peak = 0f32;
        for ch in 0..nch {
            let x = data[f * nch + ch];
            peak = peak.max(interp[ch].push_peak(&bank, x).max(x.abs()));
        }
        // L'interpolateur a environ TAPS/2 échantillons de retard : la crête est attribuée aux
        // TAPS derniers instants (ceux qui ont fait la valeur interpolée).
        if peak > ceiling {
            for w in &mut want[f.saturating_sub(TAPS)..=f] {
                *w = w.min(ceiling / peak);
            }
        }
    }
    // Minimum glissant sur la fenêtre d'anticipation (le gain baisse avant la crête), puis
    // descente linéaire sur cette fenêtre et remontée exponentielle.
    let mut env = vec![1f32; frames];
    let mut deque: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for i in 0..frames + look {
        if i < frames {
            while deque.back().map(|&j| want[j] >= want[i]).unwrap_or(false) {
                deque.pop_back();
            }
            deque.push_back(i);
        }
        if i >= look {
            let t = i - look;
            while deque.front().map(|&j| j + look < t || j < t).unwrap_or(false) {
                deque.pop_front();
            }
            env[t] = deque.front().map(|&j| want[j]).unwrap_or(1.0);
        }
    }
    let release = (-1.0 / (sample_rate * 0.08)).exp() as f32;
    let step = 1.0 / look as f32;
    let mut g = 1f32;
    for (f, &target) in env.iter().enumerate() {
        g = if target < g { (g - step).max(target) } else { target + (g - target) * release };
        for ch in 0..nch {
            data[f * nch + ch] *= g;
        }
    }
    // Garde-fou : un reste au-dessus du plafond (crête très brève) est écrêté doucement.
    for x in data.iter_mut() {
        if x.abs() > ceiling {
            *x = x.signum() * ceiling;
        }
    }
}

/// Gain (dB) qui amène `measured` sur `target` ; 0 pour un silence.
pub fn gain_db(measured: f64, target: f64) -> f64 {
    if measured.is_finite() && target.is_finite() {
        target - measured
    } else {
        0.0
    }
}

/// Normalise un son entrelacé vers `target` LUFS : gain, puis limiteur (crête vraie sous
/// `ceiling` dBTP) et, si le limiteur a mordu, des passes de rattrapage (+6 dB au plus chacune).
/// Renvoie les mesures avant et après.
pub fn normalize(data: &mut [f32], sample_rate: f64, channels: usize, target: f64, ceiling: f64) -> (Summary, Summary) {
    let before = measure(data, sample_rate, channels);
    let mut after = before;
    if !before.integrated.is_finite() {
        return (before, after);
    }
    for pass in 0..5 {
        let g = gain_db(after.integrated, target);
        if pass > 0 && g.abs() < 0.2 {
            break;
        }
        let g = if pass > 0 { g.min(6.0) } else { g };
        let k = 10f64.powf(g / 20.0) as f32;
        data.iter_mut().for_each(|x| *x *= k);
        // Les variations rapides du gain créent de nouvelles crêtes entre échantillons : on
        // relimite, plafond abaissé de l'excès, tant que la crête vraie le dépasse.
        let mut c = ceiling;
        for _ in 0..4 {
            limit(data, sample_rate, channels, c);
            after = measure(data, sample_rate, channels);
            if after.true_peak <= ceiling + 0.05 {
                break;
            }
            c -= after.true_peak - ceiling + 0.1;
        }
    }
    (before, after)
}

// --- Réglage et fichiers -------------------------------------------------------------------------

/// Cible du montage en cours : « loudness » de work/son.json (LUFS de −30 à −8), None si la
/// normalisation est désactivée (false), −14 par défaut. Mode Dérushage : désactivée.
pub fn target() -> Option<f64> {
    if crate::moments_lib::app_mode(&paths::work()) == "derush" {
        return None;
    }
    target_of(&util::read_obj(paths::work().join("son.json")))
}

pub fn target_of(sound: &Map<String, Value>) -> Option<f64> {
    match sound.get("loudness") {
        None | Some(Value::Null) => Some(DEFAULT_TARGET),
        Some(Value::Bool(false)) => None,
        Some(v) => v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).map(|x| x.clamp(-30.0, -8.0)),
    }
}

/// Son d'un fichier, en flottants 48 kHz stéréo entrelacés (toutes les pistes mélangées, comme
/// la préparation : moments_lib::audio_map).
pub fn decode(path: &Path) -> Result<Vec<f32>, String> {
    let mut args: Vec<String> = vec!["-v".into(), "error".into(), "-i".into(), paths::display(path)];
    args.extend(crate::moments_lib::audio_map(path, "son").unwrap_or_else(|| vec!["-map".into(), "0:a:0".into()]));
    args.extend(["-ac", "2", "-ar", "48000", "-f", "f32le", "-"].iter().map(|s| s.to_string()));
    let out = Command::new(paths::ffmpeg()).args(&args).stdin(Stdio::null()).output().map_err(|e| format!("ffmpeg : {e}"))?;
    if !out.status.success() {
        return Err(format!("ffmpeg n'a pas pu lire le son de {} : {}", path.display(), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

/// Mesure du son d'un fichier.
pub fn measure_file(path: &Path) -> Result<Summary, String> {
    let data = decode(path)?;
    Ok(measure(&data, SR, 2))
}

/// Remplace le son de `src` par `audio` (48 kHz stéréo, AAC 192 kb/s), image copiée, dans `dst`.
pub fn remux(src: &Path, audio: &[f32], dst: &Path) -> Result<(), String> {
    let mut child = Command::new(paths::ffmpeg())
        .args(["-v", "error", "-y", "-i"])
        .arg(src)
        .args(["-f", "f32le", "-ar", "48000", "-ac", "2", "-i", "-", "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", "-shortest"])
        .arg(dst)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg : {e}"))?;
    let mut stdin = child.stdin.take().unwrap();
    let bytes: Vec<u8> = audio.iter().flat_map(|x| x.to_le_bytes()).collect();
    let writer = std::thread::spawn(move || stdin.write_all(&bytes));
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let _ = writer.join();
    let st = child.wait().map_err(|e| e.to_string())?;
    if !st.success() {
        return Err(format!("ffmpeg (son normalisé) : {}", err.trim()));
    }
    Ok(())
}

pub fn fmt_lufs(x: f64) -> String {
    if x.is_finite() {
        format!("{x:.1} LUFS")
    } else {
        "silence".into()
    }
}

/// Normalise le son d'une vidéo vers `target` (image copiée), en place si `dst` est `src`.
/// Renvoie les mesures avant et après.
pub fn normalize_file(src: &Path, dst: &Path, target: f64) -> Result<(Summary, Summary), String> {
    let mut audio = decode(src)?;
    let before = measure(&audio, SR, 2);
    if before.integrated.is_finite() && (before.integrated - target).abs() <= 0.5 && before.true_peak <= MAX_TRUE_PEAK + 0.05 {
        if src != dst {
            std::fs::copy(src, dst).map_err(|e| e.to_string())?;
        }
        return Ok((before, before)); // déjà dans la tolérance : fichier gardé tel quel
    }
    let (_, after) = normalize(&mut audio, SR, 2, target, MAX_TRUE_PEAK - 0.3);
    let tmp = dst.with_extension("volume.mp4");
    remux(src, &audio, &tmp)?;
    let _ = std::fs::remove_file(dst);
    std::fs::rename(&tmp, dst).map_err(|e| e.to_string())?;
    // Mesure du fichier encodé (l'AAC bouge un peu la crête).
    let encoded = measure_file(dst).unwrap_or(after);
    Ok((before, encoded))
}

/// Normalisation de la vidéo rendue (render) : vers la cible du montage, résultat dans
/// work/volume.json ({fichier: {avant, apres, crete, cible}}) pour la page des téléchargements.
pub fn after_render(out: &Path) -> Result<(), String> {
    let Some(target) = target() else {
        say("Volume : normalisation désactivée (réglage du son)");
        return Ok(());
    };
    let (before, after) = normalize_file(out, out, target)?;
    say(&format!(
        "Volume du montage : {} → {} (cible {target:.0} LUFS), crête {:.1} dBTP, plage {:.1} LU",
        fmt_lufs(before.integrated),
        fmt_lufs(after.integrated),
        after.true_peak,
        after.range
    ));
    record(out, &before, &after, target);
    Ok(())
}

fn r1(x: f64) -> Value {
    if x.is_finite() {
        util::num(util::round_to(x, 1))
    } else {
        Value::Null
    }
}

/// Mesures d'un fichier gardées dans work/volume.json (clé : nom du fichier).
pub fn record(out: &Path, before: &Summary, after: &Summary, target: f64) {
    let path = paths::work().join("volume.json");
    let mut all = util::read_obj(&path);
    let name = out.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    all.insert(name, json!({"avant": r1(before.integrated), "apres": r1(after.integrated), "crete": r1(after.true_peak),
                            "plage": r1(after.range), "cible": r1(target)}));
    let _ = std::fs::write(&path, crate::pyjson::dumps_with(&Value::Object(all), crate::pyjson::Opts { indent: Some(1), ensure_ascii: false, ..Default::default() }));
}

pub fn main(args: &[String]) -> i32 {
    let Some(file) = args.first() else {
        eprintln!("Usage : auto-montage loudness <fichier> [--cible LUFS [--sortie <fichier>]]");
        return 2;
    };
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let path = Path::new(file);
    match (opt("--cible").and_then(|c| c.parse::<f64>().ok()), opt("--sortie")) {
        (Some(target), Some(out)) => match normalize_file(path, Path::new(&out), target) {
            Ok((b, a)) => {
                println!("{} → {} (crête {:.1} dBTP) : {out}", fmt_lufs(b.integrated), fmt_lufs(a.integrated), a.true_peak);
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        _ => match measure_file(path) {
            Ok(s) => {
                println!("{} : {}, plage {:.1} LU, crête vraie {:.1} dBTP", file, fmt_lufs(s.integrated), s.range, s.true_peak);
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, db: f64, secs: f64, channels: usize) -> Vec<f32> {
        let a = 10f64.powf(db / 20.0);
        let n = (secs * SR) as usize;
        (0..n).flat_map(|i| std::iter::repeat_n((a * (2.0 * PI * freq * i as f64 / SR).sin()) as f32, channels)).collect()
    }

    #[test]
    fn coefficients_match_the_48k_table() {
        // Table 1 et 2 de l'UIT-R BS.1770-4 (48 kHz).
        let p = k_prefilter(48000.0);
        assert!((p.b0 - 1.53512485958697).abs() < 1e-9 && (p.a1 + 1.69065929318241).abs() < 1e-9);
        let r = k_rlb(48000.0);
        assert!((r.a1 + 1.99004745483398).abs() < 1e-9 && (r.a2 - 0.99007225036621).abs() < 1e-9);
    }

    #[test]
    fn one_khz_sine_reads_its_level() {
        // EBU Tech 3341 : sinus de 1 kHz à −23 dBFS sur les deux canaux → −23 LUFS (±0,1).
        let s = measure(&sine(1000.0, -23.0, 20.0, 2), SR, 2);
        assert!((s.integrated + 23.0).abs() < 0.1, "{}", s.integrated);
        // Un seul canal : 3 dB de moins.
        let m = measure(&sine(1000.0, -23.0, 20.0, 1), SR, 1);
        assert!((m.integrated + 26.0).abs() < 0.1, "{}", m.integrated);
    }

    #[test]
    fn gates_ignore_silence() {
        let mut v = sine(1000.0, -20.0, 10.0, 2);
        v.extend(std::iter::repeat_n(0f32, (20.0 * SR) as usize * 2));
        let s = measure(&v, SR, 2);
        assert!((s.integrated + 20.0).abs() < 0.1, "{}", s.integrated);
        assert!(measure(&vec![0f32; 96000], SR, 2).integrated.is_infinite());
    }

    #[test]
    fn true_peak_sees_intersample_peaks() {
        // Sinus à fs/4 déphasé de 45° : échantillons à ±0,707, crête vraie 0 dBTP.
        let v: Vec<f32> = (0..48000).map(|n| ((PI / 2.0) * n as f64 + PI / 4.0).sin() as f32).collect();
        let s = measure(&v, SR, 1);
        assert!(s.true_peak > -0.2, "{}", s.true_peak);
    }

    #[test]
    fn normalize_reaches_target_under_ceiling() {
        // Voix trop basse avec des crêtes (bips forts et brefs) : cible atteinte, crête bornée.
        let mut v = sine(220.0, -32.0, 30.0, 2);
        for k in 0..20 {
            let at = (k as f64 * 1.4 * SR) as usize * 2;
            for i in 0..2400 {
                v[at + i] = if i % 2 == 0 { 0.9 } else { -0.9 };
            }
        }
        let (before, after) = normalize(&mut v, SR, 2, -14.0, -1.3);
        assert!(before.integrated < -25.0);
        assert!((after.integrated + 14.0).abs() < 0.5, "{}", after.integrated);
        assert!(after.true_peak <= -1.0, "{}", after.true_peak);
    }

    #[test]
    fn target_setting() {
        let mut m = Map::new();
        assert_eq!(target_of(&m), Some(-14.0));
        m.insert("loudness".into(), json!(false));
        assert_eq!(target_of(&m), None);
        m.insert("loudness".into(), json!(-23));
        assert_eq!(target_of(&m), Some(-23.0));
        m.insert("loudness".into(), json!(-2));
        assert_eq!(target_of(&m), Some(-8.0));
    }
}
