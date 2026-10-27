//! Transcrit les segments de parole détectés par le VAD Silero, regroupés en morceaux.
//!
//! Entrées : work/rush_16k.wav, work/vad.txt (sortie de vad-speech-segments)
//! Sortie  : work/segments.json -> [{id, start, end, text, words:[{w, a, b}]}]
//! Les temps sont en secondes, dans la timeline du rush original.
//!
//! Whisper traite toujours une fenêtre de 30 s : une phrase de 2 s transcrite seule coûte autant
//! qu'une de 30 s. Les segments consécutifs sont donc mis bout à bout dans des morceaux de 28 s au
//! plus, séparés par 1 s de silence, et un seul Whisper (modèle chargé une fois) transcrit tous
//! les morceaux : 6 fois plus rapide sur un rush de 6 min. Chaque mot est rendu à son segment
//! d'après son temps DTW (alignement sur le son, plus juste que les temps des jetons, qui
//! débordent sur le segment voisin). Whisper omet souvent une hésitation isolée (« Euh… »,
//! « C'est… ») au milieu d'un morceau : les segments restés vides sont transcrits ensuite seuls,
//! comme avant, pour ne pas perdre les ratés que le dérush doit repérer.
//!
//! Reprise (préparation mise en pause ou coupée, AM_REPRISE=1, prepare --reprendre) : les morceaux
//! déjà transcrits (work/seg/<morceau>.wav.json) sont gardés si le son, les segments et la langue
//! n'ont pas changé (work/seg/cle.txt) ; Whisper ne transcrit que les autres.

use crate::moments_lib::language_from;
use crate::progress::report_every;
use crate::pyjson::{self, float_repr};
use crate::util::round_to;
use crate::{paths, progress};
use regex::Regex;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PAD: f64 = 0.15; // son gardé autour de chaque segment
const GAP: f64 = 1.0; // silence entre deux segments d'un morceau
const CHUNK: f64 = 28.0; // longueur maximale d'un morceau (fenêtre de Whisper : 30 s)

struct Ctx {
    audio: Vec<u8>, // PCM 16 bits mono, comme readframes()
    segs: Vec<(f64, f64)>,
    seg_dir: PathBuf,
    resume: bool,
    lang: String,
}

/// Une place d'un segment dans un morceau : (début dans le morceau, fin dans le morceau, début
/// dans le rush, id du segment).
type Place = (f64, f64, f64, usize);

/// Lit un WAV PCM 16 bits : (fréquence, octets des échantillons).
fn read_wav(path: &Path) -> Result<(u32, Vec<u8>), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("{}: {e}", paths::display(path)))?;
    let sr = r.spec().sample_rate;
    let mut out = Vec::with_capacity(r.len() as usize * 2);
    for s in r.samples::<i16>() {
        let s = s.map_err(|e| e.to_string())?;
        out.extend_from_slice(&s.to_le_bytes());
    }
    Ok((sr, out))
}

fn write_wav(path: &Path, sr: u32, data: &[u8]) {
    let spec = hound::WavSpec { channels: 1, sample_rate: sr, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    if let Ok(mut w) = hound::WavWriter::create(path, spec) {
        for c in data.chunks_exact(2) {
            let _ = w.write_sample(i16::from_le_bytes([c[0], c[1]]));
        }
        let _ = w.finalize();
    }
}

/// Tranche `audio[a:b]` à la façon de Python (bornes ramenées dans le tableau).
fn slice(audio: &[u8], a: usize, b: usize) -> &[u8] {
    let b = b.min(audio.len());
    let a = a.min(b);
    &audio[a..b]
}

fn json_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.json", paths::display(path)))
}

/// Fichier déjà transcrit lors d'une préparation interrompue (sortie de Whisper complète).
fn transcribed(path: &Path) -> bool {
    matches!(crate::util::read_json(json_path(path)), Some(Value::Object(m)) if m.contains_key("transcription"))
}

/// Clé de reprise :
/// sha1(f"{len(audio)} {sr} {LANG} {PAD} {GAP} {CHUNK} {segs}") (segs : liste de tuples).
fn key(len: usize, sr: u32, lang: &str, segs: &[(f64, f64)]) -> String {
    let list: Vec<String> = segs.iter().map(|(a, b)| format!("({}, {})", float_repr(*a), float_repr(*b))).collect();
    let text = format!("{len} {sr} {lang} {} {} {} [{}]", float_repr(PAD), float_repr(GAP), float_repr(CHUNK), list.join(", "));
    format!("{}\n", hex::encode(Sha1::digest(text.as_bytes())))
}

impl Ctx {
    /// Un seul Whisper pour les fichiers pas encore transcrits (modèle chargé une fois) ; JSON
    /// complet à côté de chaque fichier (<fichier>.json), avec le temps DTW de chaque jeton.
    /// Avancement : weights[fichier] atteint quand son JSON est écrit (temps du rush pour les
    /// morceaux, dans l'ordre ; nombre de segments pour les segments seuls).
    fn whisper(&self, files: &[PathBuf], label: &str, weights: &HashMap<String, f64>, total_weight: f64, mut done: f64) -> Result<f64, String> {
        let files: Vec<&PathBuf> = files.iter().filter(|f| !(self.resume && transcribed(f))).collect();
        if files.is_empty() {
            return Ok(done);
        }
        let model = paths::whisper_dir().join("ggml-large-v3-turbo.bin");
        let mut cmd = Command::new(paths::whisper_bin("whisper-cli"));
        cmd.arg("-m").arg(&model).args(["-l", &self.lang, "-t", "4", "-mc", "0", "-ml", "1", "-sow", "-ojf", "-dtw", "large.v3.turbo"]);
        for f in &files {
            cmd.arg("-f").arg(f);
        }
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Échec de la transcription (whisper-cli ne se lance pas : {e})"))?;
        let saving = Regex::new(r"saving output to '(.+)\.json'").unwrap();
        let mut tail: Vec<String> = vec![];
        let mut reader = BufReader::new(child.stderr.take().unwrap());
        let mut buf = vec![];
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&buf).into_owned();
            tail.push(line.trim_end().to_string());
            if tail.len() > 20 {
                tail.remove(0);
            }
            if let Some(m) = saving.captures(&line) {
                if let Some(w) = weights.get(&m[1]) {
                    done = done.max(*w);
                    report_every(label, done, total_weight, 0.0);
                }
            }
        }
        let code = crate::procs::wait(&mut child);
        if code != 0 {
            progress::say(&tail.join("\n"));
            return Err(format!("Échec de la transcription (whisper-cli, code {code})"));
        }
        Ok(done)
    }

    /// Range chaque mot dans le segment qui contient son début (le plus proche sinon) et
    /// convertit ses temps en temps du rush, bornés au segment.
    fn place_words(&self, found: &[(String, f64, f64)], places: &[Place]) -> BTreeMap<usize, Vec<Value>> {
        let mut by_seg: BTreeMap<usize, Vec<(String, f64, f64)>> = BTreeMap::new();
        let mut order: Vec<usize> = vec![];
        for (text, a, b) in found {
            let dist = |p: &Place| if p.0 <= *a && *a <= p.1 { 0.0 } else { (a - p.0).abs().min((a - p.1).abs()) };
            let mut best = &places[0];
            for p in &places[1..] {
                if dist(p) < dist(best) {
                    best = p;
                }
            }
            let (pos, _end, cs, i) = *best;
            if !by_seg.contains_key(&i) {
                order.push(i);
            }
            by_seg.entry(i).or_default().push((text.clone(), cs + a - pos, cs + b - pos));
        }
        let mut out = BTreeMap::new();
        for (i, mut ws) in by_seg {
            let (s, e) = self.segs[i];
            for k in 0..ws.len() {
                let prev = if k > 0 { ws[k - 1].1 } else { s };
                ws[k].1 = ws[k].1.max(s).max(prev).min(e);
            }
            for k in 0..ws.len() {
                // Fin d'un mot : début du suivant ; pour le dernier, fin donnée par Whisper (ses
                // temps de jetons ne suivent pas le DTW : au moins 0,3 s), bornée au segment.
                ws[k].2 = if k + 1 < ws.len() { ws[k + 1].1 } else { e.min(ws[k].2.max(ws[k].1 + 0.3)) };
            }
            out.insert(i, ws.into_iter().map(|(t, a, b)| json!({"w": t, "a": round_to(a, 3), "b": round_to(b, 3)})).collect());
        }
        out
    }
}

/// Mots de la sortie de Whisper : (texte, temps DTW du début, fin), en secondes du fichier.
fn words_of(path: &Path) -> Result<Vec<(String, f64, f64)>, String> {
    let data = crate::util::read_json(json_path(path)).ok_or_else(|| format!("Sortie de Whisper illisible : {}.json", paths::display(path)))?;
    let mut out = vec![];
    for t in crate::util::arr(&data, "transcription") {
        let tokens: Vec<&Value> = crate::util::arr(t, "tokens").iter().filter(|k| !crate::util::s(k, "text").unwrap_or("").starts_with("[_")).collect();
        let text = crate::util::s(t, "text").unwrap_or("").trim().to_string();
        if text.is_empty() || tokens.is_empty() {
            continue;
        }
        let a = crate::util::fd(tokens[0], "t_dtw", -1.0);
        let offsets = t.get("offsets").cloned().unwrap_or(Value::Null);
        let a = if a >= 0.0 { a / 100.0 } else { crate::util::fd(&offsets, "from", 0.0) / 1000.0 };
        out.push((text, a, crate::util::fd(&offsets, "to", 0.0) / 1000.0));
    }
    Ok(out)
}

fn run() -> Result<(), String> {
    let work = paths::work();
    let lang = language_from(&work, "langue_rush.txt"); // écrite par prepare (moments_lib::LANGUAGES)
    let (sr, audio) = read_wav(&work.join("rush_16k.wav"))?;
    let total = audio.len() as f64 / 2.0 / sr as f64;
    let vad = fs::read_to_string(work.join("vad.txt")).map_err(|e| format!("work/vad.txt : {e}"))?;
    let re = Regex::new(r"start = ([\d.]+), end = ([\d.]+)").unwrap();
    let segs: Vec<(f64, f64)> = re
        .captures_iter(&vad)
        .map(|c| (c[1].parse::<f64>().unwrap_or(0.0) / 100.0, c[2].parse::<f64>().unwrap_or(0.0) / 100.0))
        .collect();

    let seg_dir = work.join("seg");
    crate::util::mkdirs(&seg_dir);
    let key = key(audio.len(), sr, &lang, &segs);
    let key_file = seg_dir.join("cle.txt");
    let resume = std::env::var("AM_REPRISE").as_deref() == Ok("1") && fs::read_to_string(&key_file).ok().as_deref() == Some(key.as_str());
    if !resume {
        for f in crate::util::list_dir(&seg_dir) {
            // restes d'une préparation précédente
            crate::util::remove_all(seg_dir.join(f));
        }
        let _ = fs::write(&key_file, &key);
    }
    let ctx = Ctx { audio, segs, seg_dir, resume, lang };
    let srf = sr as f64;
    let at = |t: f64| (t * srf) as usize * 2; // int(t * sr) * 2

    // Morceaux : segments consécutifs (un segment plus long que CHUNK forme un morceau à lui seul).
    let mut chunks: Vec<Vec<usize>> = vec![];
    let (mut cur, mut length) = (vec![], 0.0);
    for (i, (s, e)) in ctx.segs.iter().enumerate() {
        let piece = total.min(e + PAD) - (s - PAD).max(0.0);
        if !cur.is_empty() && length + piece > CHUNK {
            chunks.push(std::mem::take(&mut cur));
            length = 0.0;
        }
        cur.push(i);
        length += piece + GAP;
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }

    let mut jobs: Vec<(PathBuf, Vec<Place>, f64)> = vec![]; // (fichier wav, places, fin du morceau dans le rush)
    for (n, ids) in chunks.iter().enumerate() {
        let mut data: Vec<u8> = vec![];
        let mut places = vec![];
        for &i in ids {
            let (s, e) = ctx.segs[i];
            let (cs, ce) = ((s - PAD).max(0.0), total.min(e + PAD));
            let pos = data.len() as f64 / 2.0 / srf;
            data.extend_from_slice(slice(&ctx.audio, at(cs), at(ce)));
            places.push((pos, data.len() as f64 / 2.0 / srf, cs, i));
            data.extend(std::iter::repeat_n(0u8, 2 * (GAP * srf) as usize));
        }
        let path = ctx.seg_dir.join(format!("c{n:03}.wav"));
        if !(ctx.resume && transcribed(&path)) {
            write_wav(&path, sr, &data);
        }
        let last = ctx.segs[*ids.last().unwrap()].1;
        jobs.push((path, places, total.min(last + PAD)));
    }

    // Avancement : temps du rush atteint par les morceaux transcrits, sur la durée du rush.
    let already = jobs.iter().filter(|(p, _, _)| ctx.resume && transcribed(p)).map(|j| j.2).fold(0.0, f64::max);
    if already > 0.0 {
        let n = jobs.iter().filter(|(p, _, _)| ctx.resume && transcribed(p)).count();
        progress::say(&format!("Reprise de la transcription : {n} morceau(x) sur {} déjà transcrits", jobs.len()));
    }
    report_every("Transcription", already, total, 0.0);
    let mut words: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
    if !jobs.is_empty() {
        let files: Vec<PathBuf> = jobs.iter().map(|j| j.0.clone()).collect();
        let weights: HashMap<String, f64> = jobs.iter().map(|j| (paths::display(&j.0), j.2)).collect();
        ctx.whisper(&files, "Transcription", &weights, total, already)?;
        for (path, places, _) in &jobs {
            words.extend(ctx.place_words(&words_of(path)?, places));
        }
    }
    report_every("Transcription", total, total, 0.0);

    // Segments restés vides : transcrits seuls (hésitation isolée, bout de phrase).
    let empty: Vec<usize> = (0..ctx.segs.len()).filter(|i| words.get(i).is_none_or(|w| w.is_empty())).collect();
    if !empty.is_empty() {
        progress::say(&format!("{} segment(s) sans mot dans les morceaux : transcrits seuls", empty.len()));
        let mut alone: Vec<(PathBuf, Vec<Place>, f64)> = vec![];
        for (k, &i) in empty.iter().enumerate() {
            let (s, e) = ctx.segs[i];
            let (cs, ce) = ((s - PAD).max(0.0), total.min(e + PAD));
            let path = ctx.seg_dir.join(format!("s{i:03}.wav"));
            if !(ctx.resume && transcribed(&path)) {
                write_wav(&path, sr, slice(&ctx.audio, at(cs), at(ce)));
            }
            alone.push((path, vec![(0.0, ce - cs, cs, i)], (k + 1) as f64));
        }
        report_every("Segments transcrits seuls", 0.0, alone.len() as f64, 0.0);
        let files: Vec<PathBuf> = alone.iter().map(|j| j.0.clone()).collect();
        let weights: HashMap<String, f64> = alone.iter().map(|j| (paths::display(&j.0), j.2)).collect();
        ctx.whisper(&files, "Segments transcrits seuls", &weights, alone.len() as f64, 0.0)?;
        for (path, places, _) in &alone {
            words.extend(ctx.place_words(&words_of(path)?, places));
        }
    }

    let mut result = vec![];
    for (i, (s, e)) in ctx.segs.iter().enumerate() {
        let ws = words.get(&i).cloned().unwrap_or_default();
        let text = ws.iter().map(|x| x["w"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join(" ");
        progress::say(&format!("#{i} [{s:.2}-{e:.2}] {text}"));
        result.push(json!({"id": i, "start": s, "end": e, "text": text, "words": ws}));
    }
    let out = pyjson::dumps_with(&Value::Array(result), pyjson::Opts { indent: Some(1), ensure_ascii: false, sort_keys: false });
    fs::write(work.join("segments.json"), out).map_err(|e| format!("work/segments.json : {e}"))?;
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
    fn key_like_python() {
        // hashlib.sha1(f"{1000} {16000} fr {0.15} {1.0} {28.0} {[(0.0, 4.04), (5.56, 9.16)]}".encode()).hexdigest()
        let k = key(1000, 16000, "fr", &[(0.0, 4.04), (5.56, 9.16)]);
        let text = "1000 16000 fr 0.15 1.0 28.0 [(0.0, 4.04), (5.56, 9.16)]";
        assert_eq!(k, format!("{}\n", hex::encode(Sha1::digest(text.as_bytes()))));
    }
}
