//! Qui parle : locuteurs du rush (diarisation) et personne à l'image de chacun.
//!
//! Entrées : work/segments.json (transcription), work/rush_16k.wav, src/data/face.json et
//! public/rushes/rush_1080.mp4 (visages, detect_face).
//! Sorties : work/segments.json, avec « spk » (numéro du locuteur, à partir de 0) sur chaque mot et
//! chaque segment (locuteur qui y parle le plus longtemps) ; work/locuteurs.json :
//!   {"speakers": [{"id", "person", "seconds", "share", "lips", "alone", "seen"}], "method", "hold"}
//!   person  : personne de detect_face (vignette work/visages/p<n>.jpg) qui parle, ou null si elle
//!             n'est pas trouvée à l'image (voix hors champ) ;
//!   seconds : temps de parole (mots) ; share : part du temps de parole ;
//!   lips    : mouvement des lèvres de cette personne quand ce locuteur parle, rapporté à sa moyenne
//!             sur tous les locuteurs (1 = autant que d'habitude) ; alone : part des instants où elle
//!             est seule à l'image ; seen : part des instants où elle est à l'image.
//!
//! 1. Locuteurs, sans modèle : méthode de FilmCraft (crates/speech/src/diarize.rs, licence MIT ou
//!    Apache-2.0, reprise et adaptée ici) : les mots sont groupés en morceaux (nouvelle pause d'au
//!    moins 0,25 s, ou 4 s au plus ; morceau de moins de 0,7 s joint au précédent) ; chaque morceau
//!    reçoit un vecteur de voix (moyenne et écart type des MFCC 1 à 19 de ses images voisées, à
//!    30 dB au plus de la plus forte, chaque coefficient rapporté à sa dispersion sur tout le rush) ;
//!    regroupement hiérarchique par lien moyen (distance quadratique moyenne) jusqu'à ce que les
//!    deux groupes les plus proches soient à plus de THRESHOLD ; un groupe de moins de 5 % de la
//!    parole rejoint le plus proche. Locuteurs numérotés dans l'ordre où ils parlent. Une seule voix
//!    reste un seul locuteur (les morceaux d'une voix restent proches d'elle-même).
//! 2. Personne de chaque locuteur : pendant ses morceaux (jusqu'à MAX_WINDOWS fenêtres de 1,6 s),
//!    les images de la version de travail sont lues en gris et, pour chaque visage présent (face.json
//!    de l'instant), le mouvement de la bouche (différence d'une image à la suivante dans le bas du
//!    visage) est rapporté à celui du haut du visage (mouvement de la tête, bruit de compression).
//!    Une personne dont les lèvres bougent nettement plus quand ce locuteur parle que d'habitude
//!    (lips > LIPS_MIN) est la sienne (une personne par locuteur, les plus nettes d'abord) ; sinon,
//!    une personne seule à l'image pendant au moins ALONE_MIN de ses fenêtres (et qui n'est pas déjà
//!    à un autre locuteur) ; un seul locuteur : la personne la plus présente pendant qu'il parle.
//!
//! Tours de parole (turns) : lus par build_edit (cadrage « personne qui parle », fenêtres « focus »)
//! et make_moments (« turns » de chaque moment, suivis par l'aperçu de la page des moments), avec la
//! même règle : un changement de locuteur de moins de HOLD secondes ne change pas le cadrage.
//!
//! Reprise (AM_REPRISE=1) : rien n'est refait si work/locuteurs.json est plus récent que
//! work/segments.json et que les mots ont déjà leur locuteur.

use crate::progress::{report, say};
use crate::util::{self, num, round_to};
use crate::{paths, pyjson};
use serde_json::{json, Map, Value};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// Fréquence du son analysé (work/rush_16k.wav).
const SR: usize = 16_000;
/// Fenêtre (25 ms), pas (10 ms) et taille de la FFT (fenêtre complétée de zéros).
const WIN: usize = 400;
const HOP: usize = 160;
const NFFT: usize = 512;
const NBINS: usize = NFFT / 2 + 1;
const N_MELS: usize = 40;
const N_MFCC: usize = 20;

/// Découpage en morceaux (secondes) : pause qui commence un morceau, durée minimale et maximale.
const CHUNK_GAP: f64 = 0.25;
const MIN_CHUNK: f64 = 0.7;
const MAX_CHUNK: f64 = 4.0;
/// Distance au-delà de laquelle deux groupes de morceaux sont deux voix.
pub const THRESHOLD: f32 = 0.4;
const MAX_SPEAKERS: usize = 6;
/// Groupe de moins de cette part de la parole : rejoint le groupe le plus proche.
const MIN_SHARE: f32 = 0.05;

/// Fenêtres d'analyse des lèvres par locuteur, et leur durée.
const MAX_WINDOWS: usize = 24;
const WINDOW: f64 = 1.6;
/// Largeur des images lues pour les lèvres.
const LIP_W: usize = 960;
/// Lèvres d'une personne qui bougent au moins 15 % de plus que d'habitude : elle parle.
const LIPS_MIN: f64 = 1.15;
/// Personne seule à l'image pendant au moins cette part des fenêtres d'un locuteur.
const ALONE_MIN: f64 = 0.6;

/// Tenue minimale d'un cadrage « personne qui parle » (secondes).
pub const HOLD: f64 = 1.5;

// ---------------------------------------------------------------------------------------------
// Son : MFCC

/// FFT en place (radix 2, NFFT points).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (c, s) = (c as f32, s as f32);
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

fn hz_to_mel(f: f64) -> f64 {
    // échelle de Slaney : linéaire sous 1 kHz, logarithmique au-dessus
    let (f_sp, min_log_hz) = (200.0 / 3.0, 1000.0);
    let logstep = 6.4f64.ln() / 27.0;
    if f >= min_log_hz {
        min_log_hz / f_sp + (f / min_log_hz).ln() / logstep
    } else {
        f / f_sp
    }
}

fn mel_to_hz(m: f64) -> f64 {
    let (f_sp, min_log_hz) = (200.0 / 3.0, 1000.0);
    let min_log_mel = min_log_hz / f_sp;
    let logstep = 6.4f64.ln() / 27.0;
    if m >= min_log_mel {
        min_log_hz * (logstep * (m - min_log_mel)).exp()
    } else {
        f_sp * m
    }
}

/// Banc de filtres mel (N_MELS × NBINS), 0 à 8 kHz.
fn mel_filters() -> Vec<f32> {
    let fftfreqs: Vec<f64> = (0..NBINS).map(|i| i as f64 * SR as f64 / NFFT as f64).collect();
    let (lo, hi) = (hz_to_mel(0.0), hz_to_mel(SR as f64 / 2.0));
    let mel_f: Vec<f64> = (0..N_MELS + 2).map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (N_MELS + 1) as f64)).collect();
    let mut w = vec![0f32; N_MELS * NBINS];
    for m in 0..N_MELS {
        let enorm = 2.0 / (mel_f[m + 2] - mel_f[m]);
        for (k, &f) in fftfreqs.iter().enumerate() {
            let lower = (f - mel_f[m]) / (mel_f[m + 1] - mel_f[m]);
            let upper = (mel_f[m + 2] - f) / (mel_f[m + 2] - mel_f[m + 1]);
            w[m * NBINS + k] = (lower.min(upper).max(0.0) * enorm) as f32;
        }
    }
    w
}

/// Énergie (dB) et MFCC 0..N_MFCC de chaque image de 10 ms : (images, énergies, mfcc).
fn mfcc_frames(audio: &[f32]) -> (usize, Vec<f32>, Vec<f32>) {
    let frames = audio.len() / HOP;
    let filters = mel_filters();
    // bande utile de chaque filtre (calcul plus court)
    let span: Vec<(usize, usize)> = (0..N_MELS)
        .map(|m| {
            let row = &filters[m * NBINS..(m + 1) * NBINS];
            let a = row.iter().position(|&v| v > 0.0).unwrap_or(0);
            let b = row.iter().rposition(|&v| v > 0.0).map(|b| b + 1).unwrap_or(a);
            (a, b)
        })
        .collect();
    let window: Vec<f32> = (0..WIN).map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / WIN as f64).cos()) as f32).collect();
    let dct: Vec<f32> = (0..N_MFCC * N_MELS)
        .map(|i| {
            let (k, m) = (i / N_MELS, i % N_MELS);
            (std::f32::consts::PI * k as f32 * (m as f32 + 0.5) / N_MELS as f32).cos()
        })
        .collect();
    let mut energy = vec![0f32; frames];
    let mut mfcc = vec![0f32; frames * N_MFCC];
    let (mut re, mut im) = (vec![0f32; NFFT], vec![0f32; NFFT]);
    let mut pow = vec![0f32; NBINS];
    let mut lm = [0f32; N_MELS];
    for f in 0..frames {
        // images centrées (début et fin complétés de zéros)
        let base = (f * HOP) as isize - (WIN / 2) as isize;
        for i in 0..NFFT {
            re[i] = 0.0;
            im[i] = 0.0;
            if i < WIN {
                let j = base + i as isize;
                if j >= 0 && (j as usize) < audio.len() {
                    re[i] = audio[j as usize] * window[i];
                }
            }
        }
        fft(&mut re, &mut im);
        for k in 0..NBINS {
            pow[k] = re[k] * re[k] + im[k] * im[k];
        }
        let mut e = 0f32;
        for m in 0..N_MELS {
            let (a, b) = span[m];
            let v: f32 = (a..b).map(|k| filters[m * NBINS + k] * pow[k]).sum();
            e += v;
            lm[m] = (v + 1e-10).ln();
        }
        energy[f] = 10.0 * (e + 1e-10).log10();
        for k in 0..N_MFCC {
            mfcc[f * N_MFCC + k] = (0..N_MELS).map(|m| dct[k * N_MELS + m] * lm[m]).sum();
        }
        if f % 6000 == 0 {
            report("Locuteurs", f as f64 * HOP as f64 / SR as f64, (frames * HOP) as f64 / SR as f64 * 1.25);
        }
    }
    (frames, energy, mfcc)
}

// ---------------------------------------------------------------------------------------------
// Locuteurs

/// Un mot de la transcription : (début, fin) en secondes.
pub type Word = (f64, f64);

/// Morceaux de mots (plages d'indices) : nouvelle pause d'au moins CHUNK_GAP, MAX_CHUNK au plus ;
/// un morceau trop court rejoint le précédent (le premier, le suivant).
pub fn chunks(w: &[Word]) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    let mut a = 0;
    for i in 1..=w.len() {
        if i == w.len() || w[i].0 - w[i - 1].1 >= CHUNK_GAP || w[i].1 - w[a].0 > MAX_CHUNK {
            if a < i {
                out.push(a..i);
            }
            a = i;
        }
    }
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for c in out {
        let dur = w[c.end - 1].1 - w[c.start].0;
        match merged.last_mut() {
            Some(l) if dur < MIN_CHUNK => l.end = c.end,
            _ => merged.push(c),
        }
    }
    if merged.len() > 1 && w[merged[0].end - 1].1 - w[0].0 < MIN_CHUNK {
        let first = merged.remove(0);
        merged[0].start = first.start;
    }
    merged
}

fn rms_distance(a: &[f32], b: &[f32]) -> f32 {
    (a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f32>() / a.len().max(1) as f32).sqrt()
}

/// Locuteur de chaque morceau (numérotés dans l'ordre où ils parlent) et nombre de locuteurs.
pub fn cluster(audio: &[f32], words: &[Word], ch: &[std::ops::Range<usize>]) -> (Vec<usize>, usize) {
    if ch.len() < 2 {
        return (vec![0; ch.len()], usize::from(!ch.is_empty()));
    }
    let (frames, energy, mfcc) = mfcc_frames(audio);
    let frame_of = |t: f64| ((t * SR as f64 / HOP as f64).max(0.0) as usize).min(frames);
    let dim = 2 * (N_MFCC - 1);
    let mut vecs: Vec<Vec<f32>> = Vec::new();
    let mut weight: Vec<f32> = Vec::new();
    for c in ch {
        let a = frame_of(words[c.start].0);
        let b = frame_of(words[c.end - 1].1).max(a);
        let peak = energy[a..b].iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let voiced: Vec<usize> = (a..b).filter(|&f| energy[f] > peak - 30.0).collect();
        let mut v = vec![0f32; dim];
        if !voiced.is_empty() {
            let n = voiced.len() as f32;
            for k in 1..N_MFCC {
                let mean = voiced.iter().map(|&f| mfcc[f * N_MFCC + k]).sum::<f32>() / n;
                let var = voiced.iter().map(|&f| (mfcc[f * N_MFCC + k] - mean).powi(2)).sum::<f32>() / n;
                v[k - 1] = mean;
                v[N_MFCC - 1 + k - 1] = var.sqrt();
            }
        }
        vecs.push(v);
        weight.push(voiced.len() as f32);
    }
    // Chaque coefficient rapporté à sa dispersion sur toutes les images voisées du rush (mesure
    // absolue : une voix reste proche d'elle-même quel que soit le nombre de morceaux).
    let all: Vec<usize> = {
        let peak = energy.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        (0..frames).filter(|&f| energy[f] > peak - 40.0).collect()
    };
    let nf = all.len().max(1) as f32;
    for k in 1..N_MFCC {
        let mean = all.iter().map(|&f| mfcc[f * N_MFCC + k]).sum::<f32>() / nf;
        let sd = (all.iter().map(|&f| (mfcc[f * N_MFCC + k] - mean).powi(2)).sum::<f32>() / nf).sqrt().max(1e-6);
        for v in &mut vecs {
            v[k - 1] = (v[k - 1] - mean) / sd;
            v[N_MFCC - 1 + k - 1] /= sd;
        }
    }
    let labels = agglomerate(&vecs, &weight);
    let n = labels.iter().max().map(|m| m + 1).unwrap_or(0);
    (labels, n)
}

/// Regroupement hiérarchique par lien moyen (mise à jour de Lance-Williams), puis petits groupes
/// joints au plus proche ; numéros dans l'ordre d'apparition.
fn agglomerate(vecs: &[Vec<f32>], weight: &[f32]) -> Vec<usize> {
    let n = vecs.len();
    let mut d = vec![0f32; n * n];
    for i in 0..n {
        for j in i + 1..n {
            let x = rms_distance(&vecs[i], &vecs[j]);
            d[i * n + j] = x;
            d[j * n + i] = x;
        }
    }
    let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let mut alive: Vec<bool> = vec![true; n];
    let mut count = n;
    while count > 1 {
        let mut best = (f32::INFINITY, 0, 0);
        for i in 0..n {
            if !alive[i] {
                continue;
            }
            for j in i + 1..n {
                if alive[j] && d[i * n + j] < best.0 {
                    best = (d[i * n + j], i, j);
                }
            }
        }
        if best.0 > THRESHOLD && count <= MAX_SPEAKERS {
            break;
        }
        let (_, i, j) = best;
        let (si, sj) = (members[i].len() as f32, members[j].len() as f32);
        for k in 0..n {
            if alive[k] && k != i && k != j {
                let x = (si * d[k * n + i] + sj * d[k * n + j]) / (si + sj);
                d[k * n + i] = x;
                d[i * n + k] = x;
            }
        }
        let mj = std::mem::take(&mut members[j]);
        members[i].extend(mj);
        alive[j] = false;
        count -= 1;
    }
    let mut groups: Vec<Vec<usize>> = (0..n).filter(|&i| alive[i]).map(|i| members[i].clone()).collect();
    // distance moyenne entre deux groupes (morceaux d'origine)
    let link = |a: &[usize], b: &[usize]| -> f32 {
        let mut s = 0f32;
        for &i in a {
            for &j in b {
                s += rms_distance(&vecs[i], &vecs[j]);
            }
        }
        s / (a.len() * b.len()).max(1) as f32
    };
    let total: f32 = weight.iter().sum::<f32>().max(1.0);
    while groups.len() > 1 {
        let Some(si) = groups.iter().position(|g| g.iter().map(|&i| weight[i]).sum::<f32>() < MIN_SHARE * total) else { break };
        let s = groups.remove(si);
        let near = (0..groups.len()).min_by(|&a, &b| link(&s, &groups[a]).total_cmp(&link(&s, &groups[b]))).unwrap_or(0);
        groups[near].extend(s);
    }
    let mut label = vec![0usize; n];
    for (gi, g) in groups.iter().enumerate() {
        for &i in g {
            label[i] = gi;
        }
    }
    let mut order: Vec<usize> = Vec::new();
    for &l in &label {
        if !order.contains(&l) {
            order.push(l);
        }
    }
    label.iter().map(|l| order.iter().position(|o| o == l).unwrap_or(0)).collect()
}

// ---------------------------------------------------------------------------------------------
// Personne de chaque locuteur

/// Visages d'un instant de face.json : (personne, x, y, w, h).
type Face = (i64, f64, f64, f64, f64);

fn faces_near(face: &[Value], t: f64) -> Vec<Face> {
    if face.is_empty() {
        return vec![];
    }
    let i = ((t / 0.5).round().max(0.0) as usize).min(face.len() - 1);
    // face.json : un instant toutes les 0,5 s ; sinon le plus proche
    let f = if (util::fd(&face[i], "t", -1.0) - t).abs() <= 0.26 {
        &face[i]
    } else {
        face.iter().min_by(|a, b| (util::fd(a, "t", 0.0) - t).abs().total_cmp(&(util::fd(b, "t", 0.0) - t).abs())).unwrap()
    };
    util::arr(f, "faces")
        .iter()
        .filter_map(|v| {
            let a = v.as_array()?;
            let g = |k: usize| a.get(k).and_then(Value::as_f64);
            Some((a.first()?.as_i64()?, g(1)?, g(2)?, g(3)?, g(4)?))
        })
        .filter(|f| f.0 > 0)
        .collect()
}

/// Différence moyenne de deux images grises dans un rectangle (fractions de l'image).
fn mad(a: &[u8], b: &[u8], w: usize, h: usize, r: (f64, f64, f64, f64)) -> Option<f64> {
    let x0 = ((r.0 * w as f64).max(0.0) as usize).min(w);
    let y0 = ((r.1 * h as f64).max(0.0) as usize).min(h);
    let x1 = ((r.2 * w as f64).max(0.0) as usize).min(w);
    let y1 = ((r.3 * h as f64).max(0.0) as usize).min(h);
    if x1 < x0 + 3 || y1 < y0 + 2 {
        return None;
    }
    let mut s = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            s += (a[y * w + x] as i32 - b[y * w + x] as i32).unsigned_abs() as u64;
        }
    }
    Some(s as f64 / ((x1 - x0) * (y1 - y0)) as f64)
}

/// Mouvement des lèvres de chaque visage pendant [a, a + dur] : bouche (bas du visage) rapportée
/// au haut du visage, moyenne sur les images de la fenêtre.
fn lip_activity(video: &Path, w: usize, h: usize, a: f64, dur: f64, faces: &[Face]) -> Vec<(i64, f64)> {
    let out = Command::new(paths::ffmpeg())
        .args(["-v", "error", "-nostdin", "-ss", &format!("{a:.3}"), "-t", &format!("{dur:.3}"), "-i"])
        .arg(video)
        .args(["-an", "-sn", "-dn", "-vf", &format!("scale={w}:{h},format=gray"), "-f", "rawvideo", "pipe:1"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn();
    let Ok(mut child) = out else { return vec![] };
    let mut data = Vec::new();
    if let Some(o) = child.stdout.as_mut() {
        let _ = o.read_to_end(&mut data);
    }
    let _ = child.wait();
    let size = w * h;
    let frames: Vec<&[u8]> = data.chunks_exact(size).collect();
    if frames.len() < 3 {
        return vec![];
    }
    let mut res = vec![];
    for &(p, x, y, fw, fh) in faces {
        let mouth = (x - 0.25 * fw, y + 0.17 * fh, x + 0.25 * fw, y + 0.45 * fh);
        let upper = (x - 0.3 * fw, y - 0.38 * fh, x + 0.3 * fw, y - 0.12 * fh);
        let (mut sm, mut su, mut k) = (0.0, 0.0, 0);
        for pair in frames.windows(2) {
            if let (Some(m), Some(u)) = (mad(pair[0], pair[1], w, h, mouth), mad(pair[0], pair[1], w, h, upper)) {
                sm += m;
                su += u;
                k += 1;
            }
        }
        if k > 0 {
            res.push((p, (sm / k as f64) / (su / k as f64 + 1.0)));
        }
    }
    res
}

fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

/// Mesures d'un locuteur pour une personne.
#[derive(Clone, Debug, Default)]
pub struct Link {
    pub lips: Option<f64>,
    pub alone: f64,
    pub seen: f64,
}

/// Fenêtres (début) d'analyse d'un locuteur : au plus MAX_WINDOWS, réparties sur ses morceaux.
fn windows_of(spans: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let long: Vec<(f64, f64)> = spans.iter().copied().filter(|(a, b)| b - a >= 0.6).collect();
    let pick = if long.is_empty() { spans.to_vec() } else { long };
    let n = pick.len();
    let take = n.min(MAX_WINDOWS);
    (0..take)
        .map(|i| {
            let (a, b) = pick[i * n / take.max(1)];
            (a, (b - a).clamp(0.3, WINDOW))
        })
        .collect()
}

/// Personne de chaque locuteur d'après les mesures (lips, alone, seen) : voir l'en-tête.
pub fn assign(links: &[Vec<(i64, Link)>]) -> Vec<Option<i64>> {
    let n = links.len();
    let mut out: Vec<Option<i64>> = vec![None; n];
    if n == 1 {
        out[0] = links[0].iter().filter(|(_, l)| l.seen >= 0.3).max_by(|a, b| a.1.seen.total_cmp(&b.1.seen)).map(|(p, _)| *p);
        return out;
    }
    let mut pairs: Vec<(f64, usize, i64)> = vec![];
    for (s, ls) in links.iter().enumerate() {
        for (p, l) in ls {
            if let Some(r) = l.lips {
                if r >= LIPS_MIN {
                    pairs.push((r, s, *p));
                }
            }
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut used: Vec<i64> = vec![];
    for (_, s, p) in pairs {
        if out[s].is_none() && !used.contains(&p) {
            out[s] = Some(p);
            used.push(p);
        }
    }
    // Personne seule à l'image pendant que ce locuteur parle (et pas déjà prise).
    for (s, ls) in links.iter().enumerate() {
        if out[s].is_some() {
            continue;
        }
        if let Some((p, _)) = ls.iter().filter(|(p, l)| l.alone >= ALONE_MIN && !used.contains(p)).max_by(|a, b| a.1.alone.total_cmp(&b.1.alone)) {
            out[s] = Some(*p);
            used.push(*p);
        }
    }
    out
}

/// Mesures (lèvres, présence) de chaque locuteur pour chaque personne vue pendant qu'il parle.
fn measure_links(spans: &[Vec<(f64, f64)>], face: &[Value], video: &Path) -> Vec<Vec<(i64, Link)>> {
    let n = spans.len();
    let (vw, vh) = crate::moments_lib::frame_size(paths::root());
    let w = LIP_W.min(vw as usize);
    let h = ((w as f64 * vh as f64 / vw as f64 / 2.0).round() as usize * 2).max(2);
    let wins: Vec<Vec<(f64, f64)>> = spans.iter().map(|s| windows_of(s)).collect();
    let total: usize = wins.iter().map(Vec::len).sum::<usize>().max(1);
    let mut done = 0usize;
    // (locuteur, personne) -> activités ; présence
    let mut acts: Vec<std::collections::BTreeMap<i64, Vec<f64>>> = vec![Default::default(); n];
    let mut seen: Vec<std::collections::BTreeMap<i64, (usize, usize)>> = vec![Default::default(); n];
    let can_read = video.is_file() && !face.is_empty();
    for (s, ws) in wins.iter().enumerate() {
        for &(a, dur) in ws {
            let faces = faces_near(face, a + dur / 2.0);
            for f in &faces {
                let e = seen[s].entry(f.0).or_default();
                e.0 += 1;
                if faces.len() == 1 {
                    e.1 += 1;
                }
            }
            if can_read && !faces.is_empty() {
                for (p, v) in lip_activity(video, w, h, a, dur, &faces) {
                    acts[s].entry(p).or_default().push(v);
                }
            }
            done += 1;
            report("Locuteurs", 0.8 + 0.2 * done as f64 / total as f64, 1.0);
        }
    }
    // Médiane par locuteur, puis rapport à la moyenne sur les locuteurs où la personne est vue.
    let mut med: Vec<std::collections::BTreeMap<i64, f64>> = vec![Default::default(); n];
    for s in 0..n {
        for (p, v) in acts[s].iter_mut() {
            if v.len() >= 3 {
                if let Some(m) = median(v) {
                    med[s].insert(*p, m);
                }
            }
        }
    }
    let mut out = vec![];
    for s in 0..n {
        let k = wins[s].len().max(1) as f64;
        let mut ls: Vec<(i64, Link)> = seen[s]
            .iter()
            .map(|(p, (c, alone))| {
                let others: Vec<f64> = (0..n).filter_map(|o| med[o].get(p).copied()).collect();
                let lips = match (med[s].get(p), others.len()) {
                    (Some(m), c) if c >= 2 => {
                        let mean = others.iter().sum::<f64>() / c as f64;
                        (mean > 0.0).then(|| m / mean)
                    }
                    _ => None,
                };
                (*p, Link { lips, alone: *alone as f64 / k, seen: *c as f64 / k })
            })
            .collect();
        ls.sort_by(|a, b| b.1.seen.total_cmp(&a.1.seen));
        out.push(ls);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Commande

fn read_audio(path: &Path) -> Result<Vec<f32>, String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("{} : {e}", paths::display(path)))?;
    if r.spec().sample_rate as usize != SR {
        return Err(format!("{} : son à {} Hz (16 kHz attendu)", paths::display(path), r.spec().sample_rate));
    }
    Ok(r.samples::<i16>().map(|s| s.unwrap_or(0) as f32 / 32768.0).collect())
}

fn run() -> Result<(), String> {
    let work = paths::work();
    let seg_path = work.join("segments.json");
    let out_path = work.join("locuteurs.json");
    let mut segments = match util::read_json(&seg_path) {
        Some(Value::Array(a)) => a,
        _ => return Err("work/segments.json illisible (rush pas encore transcrit)".into()),
    };
    let has_spk = segments.iter().all(|s| util::arr(s, "words").iter().all(|w| w.get("spk").is_some()));
    if std::env::var("AM_REPRISE").as_deref() == Ok("1")
        && has_spk
        && util::mtime(&out_path).unwrap_or(0.0) >= util::mtime(&seg_path).unwrap_or(f64::INFINITY)
    {
        say("Locuteurs déjà trouvés (reprise).");
        return Ok(());
    }
    report("Locuteurs", 0.0, 1.0);
    // Mots de tout le rush, dans l'ordre du temps : (segment, rang, début, fin).
    let mut idx: Vec<(usize, usize, f64, f64)> = vec![];
    for (si, s) in segments.iter().enumerate() {
        for (wi, w) in util::arr(s, "words").iter().enumerate() {
            let (a, b) = (util::fd(w, "a", 0.0), util::fd(w, "b", 0.0));
            idx.push((si, wi, a, b.max(a)));
        }
    }
    idx.sort_by(|x, y| x.2.total_cmp(&y.2));
    let words: Vec<Word> = idx.iter().map(|x| (x.2, x.3)).collect();
    let ch = chunks(&words);
    let audio = read_audio(&work.join("rush_16k.wav"))?;
    let (labels, n) = cluster(&audio, &words, &ch);
    drop(audio);
    let mut spk_of = vec![0usize; words.len()];
    for (c, l) in ch.iter().zip(&labels) {
        for k in c.clone() {
            spk_of[k] = *l;
        }
    }
    // Mots et segments : « spk ».
    for (k, &(si, wi, _, _)) in idx.iter().enumerate() {
        if let Some(Value::Object(w)) = segments[si].get_mut("words").and_then(|w| w.get_mut(wi)) {
            w.insert("spk".into(), json!(spk_of[k]));
        }
    }
    for s in segments.iter_mut() {
        let mut dur = vec![0f64; n.max(1)];
        for w in util::arr(s, "words") {
            let k = w.get("spk").and_then(Value::as_u64).unwrap_or(0) as usize;
            if k < dur.len() {
                dur[k] += (util::fd(w, "b", 0.0) - util::fd(w, "a", 0.0)).max(0.01);
            }
        }
        if let Value::Object(o) = s {
            if !util::arr(&Value::Object(o.clone()), "words").is_empty() {
                let best = (0..dur.len()).max_by(|&a, &b| dur[a].total_cmp(&dur[b])).unwrap_or(0);
                o.insert("spk".into(), json!(best));
            }
        }
    }
    let text = pyjson::dumps_with(&Value::Array(segments), pyjson::Opts { indent: Some(1), ensure_ascii: false, sort_keys: false });
    std::fs::write(&seg_path, text).map_err(|e| format!("work/segments.json : {e}"))?;

    // Temps de parole et plages de chaque locuteur.
    let mut seconds = vec![0f64; n];
    let mut spans: Vec<Vec<(f64, f64)>> = vec![vec![]; n];
    for (c, &l) in ch.iter().zip(&labels) {
        let (a, b) = (words[c.start].0, words[c.end - 1].1);
        spans[l].push((a, b));
        seconds[l] += c.clone().map(|k| words[k].1 - words[k].0).sum::<f64>();
    }
    let face = match util::read_json(paths::data().join("face.json")) {
        Some(Value::Array(a)) => a,
        _ => vec![],
    };
    let links = if n > 0 { measure_links(&spans, &face, &paths::rushes().join("rush_1080.mp4")) } else { vec![] };
    let persons = assign(&links);
    let total: f64 = seconds.iter().sum::<f64>().max(1e-6);
    let speakers: Vec<Value> = (0..n)
        .map(|s| {
            let l = persons[s].and_then(|p| links[s].iter().find(|(q, _)| *q == p)).map(|x| x.1.clone()).unwrap_or_default();
            let mut o = Map::new();
            o.insert("id".into(), json!(s));
            o.insert("person".into(), persons[s].map(|p| json!(p)).unwrap_or(Value::Null));
            o.insert("seconds".into(), num(round_to(seconds[s], 1)));
            o.insert("share".into(), num(round_to(seconds[s] / total, 3)));
            o.insert("lips".into(), l.lips.map(|v| num(round_to(v, 2))).unwrap_or(Value::Null));
            o.insert("alone".into(), num(round_to(l.alone, 2)));
            o.insert("seen".into(), num(round_to(l.seen, 2)));
            // mesures pour chaque personne vue (vérification, réglages)
            o.insert(
                "candidates".into(),
                Value::Array(
                    links[s]
                        .iter()
                        .map(|(p, l)| {
                            json!({"person": p, "lips": l.lips.map(|v| num(round_to(v, 2))).unwrap_or(Value::Null),
                                   "alone": num(round_to(l.alone, 2)), "seen": num(round_to(l.seen, 2))})
                        })
                        .collect(),
                ),
            );
            Value::Object(o)
        })
        .collect();
    let out = json!({"speakers": speakers, "method": "mfcc", "hold": num(HOLD)});
    util::write_json_atomic(&out_path, &out, Some(1)).map_err(|e| format!("work/locuteurs.json : {e}"))?;
    report("Locuteurs", 1.0, 1.0);
    say(&format!(
        "{n} locuteur(s) : {}",
        (0..n)
            .map(|s| format!(
                "n° {s} {:.0} s ({})",
                seconds[s],
                persons[s].map(|p| format!("personne {p}")).unwrap_or_else(|| "pas à l'image".into())
            ))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Ok(())
}

pub fn main(_args: &[String]) -> i32 {
    match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tours de parole (build_edit, make_moments)

/// Un tour de parole, en temps du rush : [a, b), locuteur, personne à l'image (ou None).
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub a: f64,
    pub b: f64,
    pub spk: i64,
    pub person: Option<i64>,
}

/// Tours d'après les mots (début, fin, locuteur) : suites de mots d'un même locuteur ; un tour plus
/// court que HOLD ne change pas le locuteur suivi (il rejoint le précédent, le premier le suivant) ;
/// chaque tour va jusqu'au début du suivant (les silences gardent le cadrage), le premier part de 0,
/// le dernier va jusqu'à la fin du rush.
pub fn turns_of(words: &[(f64, f64, i64)], persons: &dyn Fn(i64) -> Option<i64>) -> Vec<Turn> {
    let mut runs: Vec<(f64, f64, i64)> = vec![];
    for &(a, b, s) in words {
        match runs.last_mut() {
            Some(r) if r.2 == s => r.1 = r.1.max(b),
            _ => runs.push((a, b, s)),
        }
    }
    let mut held: Vec<(f64, f64, i64)> = vec![];
    for r in runs {
        let first = held.len() == 1;
        match held.last_mut() {
            Some(h) if h.2 == r.2 || r.1 - r.0 < HOLD => h.1 = h.1.max(r.1),
            Some(h) if h.1 - h.0 < HOLD && first => {
                // premier tour trop court : il prend le locuteur du suivant
                h.1 = r.1;
                h.2 = r.2;
            }
            _ => held.push(r),
        }
    }
    let n = held.len();
    (0..n)
        .map(|i| Turn {
            a: if i == 0 { 0.0 } else { held[i].0 },
            b: if i + 1 < n { held[i + 1].0 } else { f64::INFINITY },
            spk: held[i].2,
            person: persons(held[i].2),
        })
        .collect()
}

/// Tours de parole du rush (work/segments.json et work/locuteurs.json) ; vide sans diarisation.
pub fn turns(work: &Path) -> Vec<Turn> {
    let loc = util::read_json(work.join("locuteurs.json")).unwrap_or(Value::Null);
    let map: Vec<(i64, Option<i64>)> = util::arr(&loc, "speakers")
        .iter()
        .filter_map(|s| Some((s.get("id")?.as_i64()?, s.get("person").and_then(Value::as_i64))))
        .collect();
    if map.is_empty() {
        return vec![];
    }
    let mut words: Vec<(f64, f64, i64)> = vec![];
    if let Some(Value::Array(segs)) = util::read_json(work.join("segments.json")) {
        for s in &segs {
            for w in util::arr(s, "words") {
                if let Some(k) = w.get("spk").and_then(Value::as_i64) {
                    words.push((util::fd(w, "a", 0.0), util::fd(w, "b", 0.0), k));
                }
            }
        }
    }
    words.sort_by(|x, y| x.0.total_cmp(&y.0));
    turns_of(&words, &|s| map.iter().find(|m| m.0 == s).and_then(|m| m.1))
}

/// Tours qui recouvrent [a, b] du rush, coupés à ces bornes : (début, fin, personne ou -1, locuteur).
pub fn turns_in(turns: &[Turn], a: f64, b: f64) -> Vec<(f64, f64, i64, i64)> {
    turns
        .iter()
        .filter(|t| t.b > a && t.a < b)
        .map(|t| (t.a.max(a), t.b.min(b), t.person.unwrap_or(-1), t.spk))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une « voix » de synthèse (tests de FilmCraft) : impulsions à f0 à travers deux résonances
    /// (formants), avec une enveloppe de syllabes.
    fn voice(f0: f32, formants: [(f32, f32); 2], secs: f32, seed: u32) -> Vec<f32> {
        let n = (secs * 16_000.0) as usize;
        let mut out = vec![0f32; n];
        let mut phase = 0f32;
        let mut st = [[0f32; 2]; 2];
        let mut rng = seed;
        for (i, o) in out.iter_mut().enumerate() {
            let t = i as f32 / 16_000.0;
            let jitter = 1.0 + 0.03 * (2.0 * std::f32::consts::PI * 5.0 * t).sin();
            phase += f0 * jitter / 16_000.0;
            let mut x = if phase >= 1.0 {
                phase -= 1.0;
                1.0
            } else {
                0.0
            };
            rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            x += ((rng >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.02;
            let mut y = 0f32;
            for (k, (fc, bw)) in formants.iter().enumerate() {
                let r = (-std::f32::consts::PI * bw / 16_000.0).exp();
                let a1 = 2.0 * r * (2.0 * std::f32::consts::PI * fc / 16_000.0).cos();
                let a2 = -r * r;
                let v = x + a1 * st[k][0] + a2 * st[k][1];
                st[k][1] = st[k][0];
                st[k][0] = v;
                y += v;
            }
            let env = (std::f32::consts::PI * (t * 4.0).fract()).sin().max(0.0);
            *o = y * env * 0.05;
        }
        out
    }

    #[test]
    fn deux_voix_alternees_deux_locuteurs() {
        let a = |s| voice(110.0, [(600.0, 90.0), (1100.0, 120.0)], 2.0, s);
        let b = |s| voice(230.0, [(400.0, 80.0), (2600.0, 150.0)], 2.0, s);
        let turns = [true, false, true, true, false, false, true, false];
        let mut audio = Vec::new();
        let mut words = Vec::new();
        for (k, &is_a) in turns.iter().enumerate() {
            let t0 = audio.len() as f64 / 16_000.0;
            audio.extend(if is_a { a(k as u32) } else { b(k as u32) });
            for w in 0..4 {
                let s = t0 + w as f64 * 0.5;
                words.push((s, s + 0.45));
            }
            audio.extend(vec![0f32; 8000]);
        }
        let ch = chunks(&words);
        let (labels, n) = cluster(&audio, &words, &ch);
        assert_eq!(n, 2);
        let mut spk = vec![0; words.len()];
        for (c, l) in ch.iter().zip(&labels) {
            for k in c.clone() {
                spk[k] = *l;
            }
        }
        for (k, &is_a) in turns.iter().enumerate() {
            for &s in &spk[k * 4..k * 4 + 4] {
                assert_eq!(s, if is_a { 0 } else { 1 }, "tour {k}");
            }
        }
    }

    #[test]
    fn trois_voix_trois_locuteurs() {
        let v = [
            (110.0, [(600.0, 90.0), (1100.0, 120.0)]),
            (230.0, [(400.0, 80.0), (2600.0, 150.0)]),
            (160.0, [(800.0, 100.0), (1800.0, 130.0)]),
        ];
        let order = [0, 1, 2, 0, 2, 1, 0, 1, 2];
        let (mut audio, mut words) = (Vec::new(), Vec::new());
        for (k, &i) in order.iter().enumerate() {
            let t0 = audio.len() as f64 / 16_000.0;
            audio.extend(voice(v[i].0, v[i].1, 2.0, k as u32 + 7));
            for w in 0..4 {
                let s = t0 + w as f64 * 0.5;
                words.push((s, s + 0.45));
            }
            audio.extend(vec![0f32; 8000]);
        }
        let ch = chunks(&words);
        let (labels, n) = cluster(&audio, &words, &ch);
        assert_eq!(n, 3);
        assert_eq!(labels.len(), order.len());
        for (k, &i) in order.iter().enumerate() {
            assert_eq!(labels[k], i, "tour {k}");
        }
    }

    #[test]
    fn une_voix_un_locuteur() {
        let audio = voice(140.0, [(500.0, 90.0), (1500.0, 120.0)], 12.0, 1);
        let words: Vec<Word> = (0..20).map(|i| (i as f64 * 0.6, i as f64 * 0.6 + 0.4)).collect();
        let ch = chunks(&words);
        let (labels, n) = cluster(&audio, &words, &ch);
        assert_eq!(n, 1);
        assert!(labels.iter().all(|&l| l == 0));
    }

    #[test]
    fn morceaux_coupes_aux_pauses() {
        let w = vec![(0.0, 0.5), (0.5, 1.0), (2.0, 2.2), (3.0, 3.6), (3.6, 4.4)];
        assert_eq!(chunks(&w), vec![0..3, 3..5]);
    }

    #[test]
    fn fft_d_un_son_pur() {
        let mut re: Vec<f32> = (0..NFFT).map(|i| (2.0 * std::f32::consts::PI * 32.0 * i as f32 / NFFT as f32).cos()).collect();
        let mut im = vec![0f32; NFFT];
        fft(&mut re, &mut im);
        let peak = (0..NBINS).max_by(|&a, &b| (re[a].hypot(im[a])).total_cmp(&re[b].hypot(im[b]))).unwrap();
        assert_eq!(peak, 32);
        assert!((re[32] - NFFT as f32 / 2.0).abs() < 1e-2);
    }

    #[test]
    fn tours_tenue_minimale() {
        // A parle 3 s, B 0,6 s (trop court : A garde le cadrage), A 2 s, B 3 s.
        let mut w = vec![];
        for i in 0..6 {
            w.push((i as f64 * 0.5, i as f64 * 0.5 + 0.4, 0));
        }
        w.push((3.2, 3.8, 1));
        for i in 0..4 {
            w.push((4.0 + i as f64 * 0.5, 4.4 + i as f64 * 0.5, 0));
        }
        for i in 0..6 {
            w.push((6.5 + i as f64 * 0.5, 6.9 + i as f64 * 0.5, 1));
        }
        let t = turns_of(&w, &|s| if s == 0 { Some(2) } else { None });
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].a, t[0].b, t[0].spk, t[0].person), (0.0, 6.5, 0, Some(2)));
        assert_eq!((t[1].spk, t[1].person), (1, None));
        assert!(t[1].b.is_infinite());
        let inside = turns_in(&t, 5.0, 8.0);
        assert_eq!(inside, vec![(5.0, 6.5, 2, 0), (6.5, 8.0, -1, 1)]);
    }

    #[test]
    fn personnes_des_locuteurs() {
        let l = |lips: Option<f64>, alone: f64, seen: f64| Link { lips, alone, seen };
        // Écran partagé : lèvres de la personne 2 quand le locuteur 0 parle, de la 1 pour le 1.
        let links = vec![
            vec![(1, l(Some(0.8), 0.0, 1.0)), (2, l(Some(1.4), 0.0, 1.0))],
            vec![(1, l(Some(1.3), 0.5, 1.0)), (2, l(Some(0.7), 0.0, 0.5))],
        ];
        assert_eq!(assign(&links), vec![Some(2), Some(1)]);
        // Interview : la personne 1 seule à l'image ; le locuteur 1 (hors champ) n'a pas de personne.
        let links = vec![vec![(1, l(Some(1.5), 1.0, 1.0))], vec![(1, l(Some(0.5), 1.0, 1.0))]];
        assert_eq!(assign(&links), vec![Some(1), None]);
        // Un seul locuteur : la personne la plus présente.
        let links = vec![vec![(1, l(None, 0.2, 0.4)), (3, l(None, 0.6, 0.9))]];
        assert_eq!(assign(&links), vec![Some(3)]);
    }
}
