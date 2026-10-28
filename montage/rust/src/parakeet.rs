//! Parakeet TDT 0.6B v3 (NVIDIA) en ONNX (fichiers de `istupakov/parakeet-tdt-0.6b-v3-onnx`), par
//! ONNX Runtime : préprocesseur (nemo128.onnx : 128 bandes mel, pas de 10 ms), encodeur (une sortie
//! toutes les 80 ms), puis décodage glouton TDT avec le décodeur-joint, comme onnx-asr (version
//! Python) : à chaque pas, jeton le plus probable (blanc ou non) et durée la plus probable (0 à 4
//! pas de 80 ms). Donne chaque jeton, le temps de son début et sa log-probabilité (confiance).
//!
//! La bibliothèque ONNX Runtime (`ORT_VERSION`, depuis les versions publiées de Microsoft) est
//! téléchargée par setup_whisper dans `<parakeet>/onnxruntime/` et chargée au lancement.

use crate::paths;
use ort::session::Session;
use ort::value::Tensor;
use std::fs;
use std::path::PathBuf;

pub const MODEL: &str = "parakeet-tdt-0.6b-v3";
/// Fichiers du modèle (révision fixée dans setup.rs).
pub const FILES: [&str; 6] =
    ["config.json", "vocab.txt", "nemo128.onnx", "decoder_joint-model.onnx", "encoder-model.onnx", "encoder-model.onnx.data"];
pub const ORT_VERSION: &str = "1.22.0";
const STEP: f64 = 0.08; // une sortie de l'encodeur toutes les 80 ms (10 ms × 8)
const MAX_TOKENS_PER_STEP: usize = 10;

/// Nom de la bibliothèque ONNX Runtime de ce système, dans l'archive de Microsoft (dossier lib/).
pub fn ort_lib_name() -> String {
    if cfg!(windows) {
        "onnxruntime.dll".into()
    } else if cfg!(target_os = "macos") {
        format!("libonnxruntime.{ORT_VERSION}.dylib")
    } else {
        format!("libonnxruntime.so.{ORT_VERSION}")
    }
}

pub fn ort_lib() -> PathBuf {
    paths::parakeet_dir().join("onnxruntime").join(ort_lib_name())
}

/// Archive ONNX Runtime de ce système (versions publiées de Microsoft sur GitHub).
pub fn ort_archive() -> Option<String> {
    let name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-aarch64",
        ("macos", _) => "osx-universal2",
        ("windows", "x86_64") => "win-x64",
        ("windows", "aarch64") => "win-arm64",
        _ => return None,
    };
    let ext = if cfg!(windows) { "zip" } else { "tgz" };
    Some(format!("onnxruntime-{name}-{ORT_VERSION}.{ext}"))
}

/// Sortie de Parakeet sur un son : jetons (une espace en tête ouvre un mot), temps de leur début en
/// secondes, log-probabilités.
pub struct Output {
    pub tokens: Vec<String>,
    pub timestamps: Vec<f64>,
    pub logprobs: Vec<f64>,
}

pub struct Parakeet {
    pre: Session,
    enc: Session,
    dec: Session,
    vocab: Vec<String>,
    blank: usize,
}

fn err(e: impl std::fmt::Display) -> String {
    format!("Parakeet : {e}")
}

impl Parakeet {
    /// Charge ONNX Runtime et les trois réseaux (quelques secondes, environ 2,8 Go de mémoire).
    pub fn load() -> Result<Self, String> {
        let lib = ort_lib();
        if !lib.is_file() {
            return Err(format!("ONNX Runtime absent ({}) : relancez l'installation (setup_whisper)", paths::display(&lib)));
        }
        ort::init_from(paths::display(&lib)).commit().map_err(err)?;
        let dir = paths::parakeet_dir();
        let open = |name: &str| -> Result<Session, String> {
            let path = dir.join(name);
            Session::builder().and_then(|b| b.commit_from_file(&path)).map_err(|e| format!("Parakeet ({name}) : {e}"))
        };
        // vocab.txt : « jeton id » par ligne ; ▁ marque le début d'un mot.
        let text = fs::read_to_string(dir.join("vocab.txt")).map_err(|e| format!("Parakeet (vocab.txt) : {e}"))?;
        let mut vocab: Vec<(usize, String)> = text
            .lines()
            .filter_map(|l| l.rsplit_once(' ').and_then(|(t, id)| Some((id.parse().ok()?, t.replace('\u{2581}', " ")))))
            .collect();
        vocab.sort_by_key(|v| v.0);
        let vocab: Vec<String> = vocab.into_iter().map(|v| v.1).collect();
        let blank = vocab.iter().position(|t| t == "<blk>").ok_or("Parakeet : jeton <blk> absent de vocab.txt")?;
        Ok(Parakeet { pre: open("nemo128.onnx")?, enc: open("encoder-model.onnx")?, dec: open("decoder_joint-model.onnx")?, vocab, blank })
    }

    /// Transcrit un son mono 16 kHz (échantillons de -1 à 1).
    pub fn recognize(&mut self, pcm: &[f32]) -> Result<Output, String> {
        let n = pcm.len();
        let out = self
            .pre
            .run(ort::inputs![
                "waveforms" => Tensor::from_array(([1usize, n], pcm.to_vec())).map_err(err)?,
                "waveforms_lens" => Tensor::from_array(([1usize], vec![n as i64])).map_err(err)?,
            ])
            .map_err(err)?;
        let (shape, feats) = out["features"].try_extract_tensor::<f32>().map_err(err)?;
        let shape: Vec<usize> = shape.iter().map(|&d| d as usize).collect();
        let feats = feats.to_vec();
        let lens = out["features_lens"].try_extract_tensor::<i64>().map_err(err)?.1.to_vec();
        drop(out);
        let out = self
            .enc
            .run(ort::inputs![
                "audio_signal" => Tensor::from_array((shape, feats)).map_err(err)?,
                "length" => Tensor::from_array(([1usize], lens)).map_err(err)?,
            ])
            .map_err(err)?;
        let (eshape, enc) = out["outputs"].try_extract_tensor::<f32>().map_err(err)?;
        let (dim, frames) = (eshape[1] as usize, eshape[2] as usize); // [1, 1024, T]
        let enc = enc.to_vec();
        let used = (out["encoded_lengths"].try_extract_tensor::<i64>().map_err(err)?.1[0].max(0) as usize).min(frames);
        drop(out);

        let vocab_size = self.vocab.len();
        let zero = vec![0f32; 2 * 640];
        let (mut s1, mut s2) = (zero.clone(), zero);
        let (mut tokens, mut timestamps, mut logprobs) = (vec![], vec![], vec![]);
        let mut last = self.blank as i32;
        let (mut t, mut emitted) = (0usize, 0usize);
        while t < used {
            let frame: Vec<f32> = (0..dim).map(|c| enc[c * frames + t]).collect();
            let out = self
                .dec
                .run(ort::inputs![
                    "encoder_outputs" => Tensor::from_array(([1usize, dim, 1], frame)).map_err(err)?,
                    "targets" => Tensor::from_array(([1usize, 1], vec![last])).map_err(err)?,
                    "target_length" => Tensor::from_array(([1usize], vec![1i32])).map_err(err)?,
                    "input_states_1" => Tensor::from_array(([2usize, 1, 640], s1.clone())).map_err(err)?,
                    "input_states_2" => Tensor::from_array(([2usize, 1, 640], s2.clone())).map_err(err)?,
                ])
                .map_err(err)?;
            let logits = out["outputs"].try_extract_tensor::<f32>().map_err(err)?.1;
            let argmax = |v: &[f32]| v.iter().enumerate().fold((0, f32::MIN), |b, (i, &x)| if x > b.1 { (i, x) } else { b }).0;
            let (lg, durations) = logits.split_at(vocab_size);
            let token = argmax(lg);
            let step = argmax(durations);
            if token != self.blank {
                let max = lg[token];
                let sum: f64 = lg.iter().map(|&x| ((x - max) as f64).exp()).sum();
                logprobs.push(-sum.ln());
                tokens.push(self.vocab[token].clone());
                timestamps.push(STEP * t as f64);
                last = token as i32;
                s1 = out["output_states_1"].try_extract_tensor::<f32>().map_err(err)?.1.to_vec();
                s2 = out["output_states_2"].try_extract_tensor::<f32>().map_err(err)?.1.to_vec();
                emitted += 1;
            }
            if step > 0 {
                t += step;
                emitted = 0;
            } else if token == self.blank || emitted == MAX_TOKENS_PER_STEP {
                t += 1;
                emitted = 0;
            }
        }
        Ok(Output { tokens, timestamps, logprobs })
    }
}
