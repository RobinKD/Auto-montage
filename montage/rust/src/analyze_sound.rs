//! Recherche des bruits parasites dans la version de travail (page des moments, « Son »).
//!
//! Sur l'audio d'origine (work/rush_16k.wav), dans les parties gardées au montage
//! (src/data/edit.json) :
//!   - le bruit de fond : niveau des passages sans parole (souffle, ronflement, pièce) ;
//!   - les bruits parasites : sons très brefs qui ressortent nettement de ce qui les entoure
//!     (clics, claquements de bouche, chocs, coups sur le micro), avec leur instant dans la
//!     version de travail et dans le rush.
//! Sortie : work/son_analyse.json. Les bruits à atténuer et le nettoyage choisis sur la page
//! vont dans work/son.json, appliqués par build_edit.
//!
//! (Calculs de numpy refaits à la main : fenêtre de Hann, FFT réelle, médiane glissante,
//! percentile à interpolation linéaire.)

use crate::util::{self, fd, round_to};
use crate::{paths, procs, pyjson};
use serde_json::{json, Value};
use std::f64::consts::PI;

const FPS: f64 = 30.0;
const HOP: f64 = 0.02; // trames de 20 ms

fn load_audio() -> Result<(Vec<f64>, usize), String> {
    let work = paths::work();
    let path = work.join("rush_16k.wav");
    if !path.exists() {
        let rush_rel = util::read_text(work.join("rush.txt")).ok_or("work/rush.txt introuvable")?;
        let rush = paths::root().join(rush_rel);
        let args: Vec<String> = ["-v", "error", "-y", "-i", &paths::display(&rush), "-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", &paths::display(&path)]
            .iter()
            .map(|s| s.to_string())
            .collect();
        if procs::call(paths::ffmpeg(), &args) != 0 {
            return Err("Échec de l'extraction du son du rush".into());
        }
    }
    let mut r = hound::WavReader::open(&path).map_err(|e| e.to_string())?;
    let sr = r.spec().sample_rate as usize;
    // float32 comme numpy (valeurs exactes en f64)
    let x: Vec<f64> = r.samples::<i16>().map(|s| (s.unwrap_or(0) as f32 / 32768.0) as f64).collect();
    Ok((x, sr))
}

/// `np.percentile(v, q)` (interpolation linéaire).
fn percentile(v: &[f64], q: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pos = q / 100.0 * (s.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(s.len() - 1);
    let frac = pos - lo as f64;
    s[lo] + (s[hi] - s[lo]) * frac
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// FFT complexe sur place (taille puissance de 2).
fn fft(re: &mut [f64], im: &mut [f64]) {
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
        let ang = -2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((ang * k as f64).cos(), (ang * k as f64).sin());
                let (a, b) = (start + k, start + k + len / 2);
                let (xr, xi) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
        }
        len <<= 1;
    }
}

fn run() -> Result<(), String> {
    let work = paths::work();
    let (x, sr) = load_audio()?;
    let srf = sr as f64;
    let hop = (srf * HOP) as usize;
    let n = x.len() / hop;
    let db: Vec<f64> = (0..n)
        .map(|i| {
            let m = x[i * hop..(i + 1) * hop].iter().map(|v| v * v).sum::<f64>() / hop as f64;
            20.0 * (m.sqrt() + 1e-6).log10()
        })
        .collect();
    let segments = util::read_json(work.join("segments.json")).ok_or("work/segments.json illisible")?;
    let edit = util::read_json(paths::data().join("edit.json")).ok_or("src/data/edit.json illisible")?;

    // Trames de parole (mots transcrits, avec une marge) : le reste est « hors parole ».
    let mut speech = vec![false; n];
    for s in segments.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        for w in util::arr(s, "words") {
            let a = ((fd(w, "a", 0.0) - 0.08) / HOP) as i64; // int() : vers zéro
            let b = ((fd(w, "b", 0.0) + 0.08) / HOP) as i64;
            let (lo, hi) = (a.max(0) as usize, b.min(n as i64).max(0) as usize);
            for v in speech.iter_mut().take(hi).skip(lo) {
                *v = true; // (pour le bruit de fond)
            }
        }
    }

    // Parties gardées (temps du rush) et instant correspondant dans la version de travail.
    let kept: Vec<(f64, f64, f64, Value)> = util::arr(&edit, "clips")
        .iter()
        .map(|c| {
            let tb = fd(c, "trimBefore", 0.0);
            (tb / FPS, (tb + fd(c, "durationInFrames", 0.0)) / FPS, fd(c, "from", 0.0) / FPS, c.get("seg").cloned().unwrap_or(Value::Null))
        })
        .collect();

    // Bruit de fond : niveau typique des passages sans parole (tout le rush).
    let quiet: Vec<f64> = db.iter().zip(&speech).filter(|(_, s)| !**s).map(|(d, _)| *d).collect();
    let floor = if quiet.len() > 50 { percentile(&quiet, 30.0) } else { percentile(&db, 10.0) };
    let (level, advice) = if floor < -60.0 {
        ("très faible", 0)
    } else if floor < -50.0 {
        ("faible", 1)
    } else if floor < -40.0 {
        ("audible", 2)
    } else {
        ("fort", 3)
    };

    // Bruits parasites, dans les parties gardées : sons très brefs qui ressortent nettement de
    // ce qui les entoure (± 0,4 s), même pendant la parole :
    //   - clics, claquements de bouche, grésillements : aigus (3,5-8 kHz) ;
    //   - chocs, coups sur le micro, pied de table : graves (< 150 Hz) ;
    //   - bruits hors des mots (respiration forte, objet) : niveau global.
    let (win, step) = (512usize, sr / 100); // analyse toutes les 10 ms
    let hann: Vec<f64> = (0..win).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / (win - 1) as f64).cos()).collect();
    let bins: Vec<f64> = (0..=win / 2).map(|k| k as f64 * srf / win as f64).collect();
    let mut frames: Vec<(&str, f64, f64, f64, f64, f64, Value)> = vec![];
    for (a, b, start, seg) in &kept {
        let i0 = (a * srf) as i64;
        let i1 = (x.len() as i64 - win as i64).min((b * srf) as i64);
        if i1 - i0 < (win * 4) as i64 {
            continue;
        }
        let idx: Vec<usize> = (i0..i1).step_by(step).map(|i| i as usize).collect();
        let mut hi = Vec::with_capacity(idx.len());
        let mut lo = Vec::with_capacity(idx.len());
        let (mut re, mut im) = (vec![0.0; win], vec![0.0; win]);
        for &i in &idx {
            for k in 0..win {
                re[k] = x[i + k] * hann[k];
                im[k] = 0.0;
            }
            fft(&mut re, &mut im);
            let (mut h, mut l) = (0.0, 0.0);
            for (k, f) in bins.iter().enumerate() {
                let p = re[k] * re[k] + im[k] * im[k];
                if *f >= 3500.0 {
                    h += p;
                }
                if *f < 150.0 {
                    l += p;
                }
            }
            hi.push(10.0 * (h + 1e-9).log10());
            lo.push(10.0 * (l + 1e-9).log10());
        }
        let k = 40usize; // ± 0,4 s
        for (name, band, rise) in [("clic", &hi, 18.0), ("choc", &lo, 18.0)] {
            let len = band.len();
            let padded: Vec<f64> = (0..len + 2 * k).map(|p| band[p.saturating_sub(k).min(len - 1)]).collect();
            let rf: Vec<f64> = (0..len).map(|j| median(&mut padded[j..j + 2 * k + 1].to_vec())).collect();
            let hot: Vec<bool> = (0..len).map(|j| band[j] > rf[j] + rise).collect();
            let mut j = 0;
            while j < len {
                if !hot[j] {
                    j += 1;
                    continue;
                }
                let mut e = j;
                while e < len && hot[e] {
                    e += 1;
                }
                // Isolé : 30 ms au plus, et nettement au-dessus de ce qui précède et suit de près
                // (une consonne monte et dure ; un clic retombe aussitôt).
                let around = band[j.saturating_sub(4)].max(band[(len - 1).min(e + 3)]);
                let top = band[j..e].iter().cloned().fold(f64::MIN, f64::max);
                if e - j <= 3 && top > around + 12.0 {
                    let t = idx[j] as f64 / srf;
                    let over = (j..e).map(|q| band[q] - rf[q]).fold(f64::MIN, f64::max);
                    frames.push((name, t, (e - j) as f64 * 0.01, over, *start, *a, seg.clone()));
                }
                j = e;
            }
        }
    }
    let mut events: Vec<Value> = frames
        .into_iter()
        .map(|(name, t, dur, over, start, a, seg)| {
            json!({"src": [round_to((t - 0.02).max(0.0), 2), round_to(t + dur + 0.03, 2)],
                   "at": round_to(start + t - a, 2), "seg": seg, "dur": round_to(dur.max(0.01), 2),
                   "peak": round_to(over, 1),
                   "kind": if name == "clic" { "clic ou claquement" } else { "choc ou coup sur le micro" }})
        })
        .collect();
    // Un même bruit vu dans les deux bandes : gardé une fois (le plus marqué).
    let peak = |e: &Value| fd(e, "peak", 0.0);
    events.sort_by(|a, b| peak(b).partial_cmp(&peak(a)).unwrap());
    let mut unique: Vec<Value> = vec![];
    for e in events {
        if unique.iter().all(|u| (fd(&e, "at", 0.0) - fd(u, "at", 0.0)).abs() > 0.15) {
            unique.push(e);
        }
    }
    let mut events: Vec<Value> = unique.into_iter().take(20).collect(); // les 20 plus marqués
    events.sort_by(|a, b| fd(a, "at", 0.0).partial_cmp(&fd(b, "at", 0.0)).unwrap());
    let n_events = events.len();
    let out = json!({"floor": round_to(floor, 1), "level": level, "advice": advice, "events": events});
    let text = pyjson::dumps_with(&out, pyjson::Opts { indent: Some(1), ensure_ascii: false, sort_keys: false });
    std::fs::write(work.join("son_analyse.json"), text).map_err(|e| e.to_string())?;
    println!("Bruit de fond {level} ({floor:.0} dB), {n_events} bruits parasites possibles");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_like_numpy() {
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 30.0), 1.9);
        assert_eq!(percentile(&[5.0], 10.0), 5.0);
    }

    #[test]
    fn fft_matches_dft() {
        let n = 16;
        let x: Vec<f64> = (0..n).map(|i| ((i * 7 % 5) as f64) - 2.0).collect();
        let (mut re, mut im) = (x.clone(), vec![0.0; n]);
        fft(&mut re, &mut im);
        for k in 0..n {
            let (mut r, mut i) = (0.0, 0.0);
            for (t, v) in x.iter().enumerate() {
                let a = -2.0 * PI * (k * t) as f64 / n as f64;
                r += v * a.cos();
                i += v * a.sin();
            }
            assert!((r - re[k]).abs() < 1e-9 && (i - im[k]).abs() < 1e-9);
        }
    }
}
