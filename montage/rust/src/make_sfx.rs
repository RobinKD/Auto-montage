//! Synthétise des effets sonores provisoires dans public/sfx/.
//!
//! Ils peuvent être remplacés par de vrais fichiers en gardant les mêmes noms :
//! bop.wav, key-1.wav..key-4.wav, cash.wav, et les sons courants des vidéos courtes :
//! whoosh, pop, ding, boum, montee, glitch, photo, faux, juste (liste : moments_lib::SOUNDS).
//!
//! (Synthèse de la version Python refaite sans numpy ; le générateur de hasard n'est pas celui de
//! numpy : les bruits diffèrent d'un tirage, pas leur caractère ni les durées.)

use crate::moments_lib::sfx_dir;
use std::f64::consts::PI;
use std::path::Path;

const SR: f64 = 48000.0;

/// Générateur de hasard (xoshiro256**, graine fixe : mêmes sons à chaque fois).
struct Rng {
    s: [u64; 4],
    spare: Option<f64>,
}

impl Rng {
    fn new(seed: u64) -> Rng {
        // splitmix64 pour remplir l'état
        let mut z = seed;
        let mut s = [0u64; 4];
        for v in s.iter_mut() {
            z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            *v = x ^ (x >> 31);
        }
        Rng { s, spare: None }
    }
    fn next(&mut self) -> u64 {
        let r = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        r
    }
    /// Uniforme dans [0, 1).
    fn random(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.random()
    }
    /// Loi normale centrée réduite (Box-Muller).
    fn normal(&mut self) -> f64 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        let u1 = 1.0 - self.random();
        let u2 = self.random();
        let r = (-2.0 * u1.ln()).sqrt();
        self.spare = Some(r * (2.0 * PI * u2).sin());
        r * (2.0 * PI * u2).cos()
    }
    fn normals(&mut self, n: usize) -> Vec<f64> {
        (0..n).map(|_| self.normal()).collect()
    }
}

fn save(out: &Path, name: &str, x: &[f64]) {
    let path = out.join(name);
    if path.exists() {
        // ne jamais écraser un vrai bruitage déposé à la main
        return;
    }
    let peak = x.iter().fold(0.0f64, |m, v| m.max(v.abs())) + 1e-9;
    let spec = hound::WavSpec { channels: 1, sample_rate: SR as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    match hound::WavWriter::create(&path, spec) {
        Ok(mut w) => {
            for v in x {
                let _ = w.write_sample((v / peak * 0.9 * 32767.0) as i16); // astype(int16) : vers zéro
            }
            let _ = w.finalize();
        }
        Err(e) => eprintln!("{name} : {e}"),
    }
}

/// Instants des échantillons d'une durée (np.arange(int(SR * dur)) / SR).
fn t(dur: f64) -> Vec<f64> {
    (0..(SR * dur) as usize).map(|i| i as f64 / SR).collect()
}

/// Phase d'une fréquence variable : 2π cumsum(freq) / SR.
fn phase(freq: &[f64]) -> Vec<f64> {
    let mut acc = 0.0;
    freq.iter()
        .map(|f| {
            acc += f;
            2.0 * PI * acc / SR
        })
        .collect()
}

/// `np.convolve(x, np.ones(m) / m, mode="same")` : moyenne glissante centrée.
fn smooth(x: &[f64], m: usize) -> Vec<f64> {
    let n = x.len();
    let off = (m - 1) / 2;
    (0..n)
        .map(|i| {
            // sortie « same » i = sortie complète i + off = somme des x[j], j dans [i+off-m+1, i+off]
            let hi = (i + off).min(n - 1);
            let lo = (i + off + 1).saturating_sub(m);
            x[lo..=hi].iter().sum::<f64>() / m as f64
        })
        .collect()
}

/// Passe-bande (biquad) à fréquence centrale variable (un f0 par échantillon).
fn bandpass(x: &[f64], f0: &[f64], q: f64) -> Vec<f64> {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    let mut y = vec![0.0; x.len()];
    for n in 0..x.len() {
        let w = 2.0 * PI * f0[n] / SR;
        let alpha = w.sin() / (2.0 * q);
        let cw = w.cos();
        let a0 = 1.0 + alpha;
        let out = (alpha * x[n] - alpha * x2 - (-2.0 * cw) * y1 - (1.0 - alpha) * y2) / a0;
        x2 = x1;
        x1 = x[n];
        y2 = y1;
        y1 = out;
        y[n] = out;
    }
    y
}

fn ramp(tt: &[f64], attack: f64) -> Vec<f64> {
    tt.iter().map(|v| (v / attack).min(1.0)).collect()
}

fn sign(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Somme de partiels amortis : Σ a·sin(2πft)·exp(-t·(base + f/div)).
fn bell(tt: &[f64], partials: &[(f64, f64)], base: f64, div: f64) -> Vec<f64> {
    tt.iter().map(|v| partials.iter().map(|(f, a)| a * (2.0 * PI * f * v).sin() * (-v * (base + f / div)).exp()).sum()).collect()
}

pub fn make(out: &Path) {
    let mut rng = Rng::new(7);

    // "Bop" : sinus court dont la hauteur descend, attaque douce.
    let tt = t(0.14);
    let freq: Vec<f64> = tt.iter().map(|v| 900.0 * (-v * 18.0).exp() + 260.0).collect();
    let ph = phase(&freq);
    let bop: Vec<f64> = tt.iter().zip(&ph).map(|(v, p)| p.sin() * (v / 0.004).min(1.0) * (-v * 28.0).exp()).collect();
    save(out, "bop.wav", &bop);

    // Frappe clavier : bruit filtré très court + petit "thock" grave.
    for i in 1..5 {
        let fi = i as f64;
        let tt = t(0.06);
        let noise = smooth(&rng.normals(tt.len()), 3);
        let key: Vec<f64> = tt
            .iter()
            .zip(&noise)
            .map(|(v, nz)| nz * (-v * (260.0 + 40.0 * fi)).exp() + (2.0 * PI * (140.0 + 25.0 * fi) * v).sin() * (-v * 90.0).exp() * 0.6)
            .collect();
        save(out, &format!("key-{i}.wav"), &key);
    }

    // Machine à cash : "ka" mécanique puis "ching" (cloche inharmonique).
    let ka = t(0.08);
    let mut cash: Vec<f64> = ka.iter().map(|v| rng.normal() * (-v * 70.0).exp() * 0.5).collect();
    let ching = t(1.1);
    let partials = [(2093.0, 1.0), (2637.0, 0.6), (3136.0, 0.45), (4186.0, 0.3), (5274.0, 0.2)];
    let b = bell(&ching, &partials, 3.0, 1500.0);
    cash.extend(std::iter::repeat_n(0.0, (SR * 0.05) as usize));
    cash.extend(ching.iter().zip(&b).map(|(v, x)| x * (v / 0.002).min(1.0) * 0.8));
    save(out, "cash.wav", &cash);

    // Whoosh : souffle dont la bande monte puis redescend (passage rapide, transition).
    let tt = t(0.6);
    let shape: Vec<f64> = tt.iter().map(|v| (PI * v / 0.6).sin().powi(2)).collect();
    let f0: Vec<f64> = shape.iter().map(|s| 500.0 + 2500.0 * s).collect();
    let w = bandpass(&rng.normals(tt.len()), &f0, 1.5);
    save(out, "whoosh.wav", &w.iter().zip(&shape).map(|(a, s)| a * s).collect::<Vec<_>>());

    // Pop : bulle qui éclate (sinus très court qui chute, petit clic d'attaque).
    let tt = t(0.09);
    let freq: Vec<f64> = tt.iter().map(|v| 1300.0 * (-v * 45.0).exp() + 300.0).collect();
    let mut pop: Vec<f64> = tt.iter().zip(phase(&freq)).map(|(v, p)| p.sin() * (-v * 55.0).exp()).collect();
    for v in pop.iter_mut().take(60) {
        *v += rng.normal() * 0.4;
    }
    save(out, "pop.wav", &pop);

    // Ding : petite cloche claire (bonne idée, conseil).
    let tt = t(1.3);
    let partials = [(1568.0, 1.0), (3136.0, 0.35), (4704.0, 0.15), (2350.0, 0.2)];
    let d = bell(&tt, &partials, 2.5, 2500.0);
    save(out, "ding.wav", &d.iter().zip(ramp(&tt, 0.003)).map(|(a, r)| a * r).collect::<Vec<_>>());

    // Boum : impact grave (moment fort), pitch qui tombe et souffle étouffé.
    let tt = t(1.1);
    let freq: Vec<f64> = tt.iter().map(|v| 110.0 * (-v * 6.0).exp() + 42.0).collect();
    let ph = phase(&freq);
    let thud = smooth(&rng.normals(tt.len()), 40);
    let boum: Vec<f64> = (0..tt.len())
        .map(|i| {
            let v = tt[i];
            let body = (2.2 * ph[i].sin()).tanh() * (-v * 3.2).exp();
            (body + thud[i] * (-v * 25.0).exp() * 1.5) * (v / 0.004).min(1.0)
        })
        .collect();
    save(out, "boum.wav", &boum);

    // Montée : tension qui monte (souffle et ton qui grimpent), coupée net à la fin.
    let tt = t(1.6);
    let rise: Vec<f64> = tt.iter().map(|v| (v / 1.6).powi(2)).collect();
    let tone: Vec<f64> = phase(&rise.iter().map(|r| 180.0 + 1000.0 * r).collect::<Vec<_>>()).iter().map(|p| p.sin() * 0.35).collect();
    let air = bandpass(&rng.normals(tt.len()), &rise.iter().map(|r| 400.0 + 4000.0 * r).collect::<Vec<_>>(), 3.0);
    let mut mont: Vec<f64> = (0..tt.len()).map(|i| (tone[i] + air[i]) * rise[i]).collect();
    let n = mont.len();
    for k in 0..200 {
        mont[n - 200 + k] *= 1.0 - k as f64 / 199.0; // np.linspace(1, 0, 200)
    }
    save(out, "montee.wav", &mont);

    // Glitch : coupures numériques (morceaux de signaux carrés et de bruit, hachés).
    let mut glitch = vec![];
    for k in 0..9 {
        let d = rng.uniform(0.02, 0.05);
        let tt = t(d);
        if k % 3 == 2 {
            glitch.extend(std::iter::repeat_n(0.0, tt.len()));
            continue;
        }
        let f = rng.uniform(200.0, 1800.0);
        let nz: Vec<f64> = (0..tt.len()).map(|_| (rng.normal() * 3.0).round_ties_even() / 3.0).collect();
        let amp = rng.uniform(0.5, 1.0);
        glitch.extend(tt.iter().zip(&nz).map(|(v, z)| (sign((2.0 * PI * f * v).sin()) * 0.6 + z * 0.4) * amp));
    }
    save(out, "glitch.wav", &glitch);

    // Déclencheur photo : deux claquements mécaniques rapprochés.
    let mut clack = |dur: f64, decay: f64, color: usize| -> Vec<f64> {
        let tt = t(dur);
        let nz = smooth(&rng.normals(tt.len()), color);
        tt.iter().zip(&nz).map(|(v, z)| z * (-v * decay).exp()).collect()
    };
    let mut photo = clack(0.05, 120.0, 2);
    photo.extend(std::iter::repeat_n(0.0, (SR * 0.06) as usize));
    photo.extend(clack(0.08, 80.0, 4).iter().map(|v| v * 0.8));
    save(out, "photo.wav", &photo);

    // Faux : buzzer de mauvaise réponse (deux tons graves légèrement désaccordés).
    let tt = t(0.65);
    let buzz: Vec<f64> = tt.iter().map(|v| sign((2.0 * PI * 150.0 * v).sin()) + sign((2.0 * PI * 156.0 * v).sin())).collect();
    let faux: Vec<f64> = smooth(&buzz, 6).iter().zip(&tt).map(|(b, v)| b * (v / 0.01).min(1.0) * ((0.65 - v) / 0.05).min(1.0)).collect();
    save(out, "faux.wav", &faux);

    // Juste : carillon de bonne réponse (deux notes montantes).
    let note = |f: f64, dur: f64| -> Vec<f64> {
        t(dur)
            .iter()
            .map(|v| ((2.0 * PI * f * v).sin() + 0.3 * (2.0 * PI * 2.0 * f * v).sin()) * (-v * 5.0).exp() * (v / 0.003).min(1.0))
            .collect()
    };
    let (first, second) = (note(1047.0, 0.7), note(1568.0, 0.9));
    let mut juste = vec![0.0; SR as usize];
    for (i, v) in first.iter().enumerate() {
        juste[i] += v;
    }
    let off = (SR * 0.11) as usize;
    for (i, v) in second.iter().enumerate().take(juste.len() - off) {
        juste[off + i] += v;
    }
    save(out, "juste.wav", &juste);
}

pub fn main(_args: &[String]) -> i32 {
    let out = sfx_dir();
    crate::util::mkdirs(&out);
    make(&out);
    let names: Vec<String> = crate::util::list_dir(&out).iter().map(|n| format!("'{n}'")).collect();
    println!("ok [{}]", names.join(", "));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_like_numpy_same() {
        // np.convolve([1,2,3,4,5], np.ones(3)/3, mode="same") = [1, 2, 3, 4, 3]
        let s = smooth(&[1.0, 2.0, 3.0, 4.0, 5.0], 3);
        let want = [1.0, 2.0, 3.0, 4.0, 3.0];
        assert!(s.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-12));
        // np.convolve([1,2,3,4,5], np.ones(2)/2, mode="same") = [0.5, 1.5, 2.5, 3.5, 4.5]
        let s = smooth(&[1.0, 2.0, 3.0, 4.0, 5.0], 2);
        let want = [0.5, 1.5, 2.5, 3.5, 4.5];
        assert!(s.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-12));
    }
}
