//! Transcrit les segments de parole détectés par le VAD Silero, regroupés en morceaux.
//!
//! Entrées : work/rush_16k.wav, work/vad.txt (sortie de vad-speech-segments)
//! Sortie  : work/segments.json -> [{id, start, end, text, words:[{w, a, b, p}]}]
//! Les temps sont en secondes, dans la timeline du rush original ; p : confiance du modèle dans le
//! mot (0 à 1, la plus faible de ses jetons), sous moments_lib::DOUBT le mot est « à vérifier ».
//!
//! Parakeet TDT 0.6B v3 (NVIDIA, ONNX Runtime : parakeet.rs) transcrit les segments consécutifs mis
//! bout à bout dans des morceaux de 28 s au plus, séparés par 1 s de silence (sur le rush Wikimania :
//! 5 fois plus vite que Whisper large-v3-turbo, secours compris, et temps des mots deux fois plus
//! justes). Chaque mot est rendu à son segment d'après le temps de son premier jeton.
//! Parakeet choisit la langue seul : sur un accent marqué, un passage peut sortir traduit en anglais
//! (ou dans un autre alphabet). Ces segments sont refaits seuls, précédés de deux segments sûrs dans la
//! langue du rush, qui l'y ramènent ; ceux qui restent dans l'autre langue y sont vraiment. Parakeet
//! n'écrit pas les hésitations au milieu d'un morceau : les segments restés vides sont transcrits
//! ensuite seuls par Whisper (large-v3-turbo q8_0), forcé dans la langue du rush (« Euh… », bout de
//! phrase).
//!
//! Reprise (préparation mise en pause ou coupée, AM_REPRISE=1, prepare --reprendre) : les morceaux
//! déjà transcrits (work/seg/<morceau>.wav.json) sont gardés si le son, les segments, la langue et le
//! modèle n'ont pas changé (work/seg/cle.txt) ; seuls les autres sont transcrits.

use crate::moments_lib::{language_from, DOUBT};
use crate::parakeet::{Parakeet, MODEL};
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
/// Fils d'exécution de Whisper au plus : au-delà, il ne va presque plus vite (mémoire).
const MAX_THREADS: usize = 8;

/// Fils de Whisper : tous les cœurs (les étapes de la préparation se suivent, rien d'autre ne
/// tourne), MAX_THREADS au plus. available_parallelism suit sous Linux l'affinité et le quota de
/// processeur du cgroup (conteneur).
///
/// Un seul Whisper : plusieurs à la fois sur une machine à beaucoup de cœurs ne donneraient pas
/// le même texte. Whisper reprend, quand un passage est douteux, avec un tirage au hasard dont
/// l'état passe d'un fichier au suivant ; le texte d'un morceau dépend donc des morceaux transcrits
/// avant lui dans le même whisper-cli (mesuré : 4 morceaux transcrits seuls, 8 % de mots changés).
fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, MAX_THREADS)
}

struct Ctx {
    audio: Vec<u8>, // PCM 16 bits mono, comme readframes()
    segs: Vec<(f64, f64)>,
    seg_dir: PathBuf,
    resume: bool,
    lang: String,
    model: Option<Parakeet>, // chargé au premier son à transcrire
}

/// Un mot trouvé : (texte, début, fin, confiance), en secondes du fichier transcrit.
type Found = (String, f64, f64, f64);

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

/// Fichier déjà transcrit lors d'une préparation interrompue (sortie complète de Whisper,
/// « transcription », ou de Parakeet, « parakeet »).
fn transcribed(path: &Path, key: &str) -> bool {
    matches!(crate::util::read_json(json_path(path)), Some(Value::Object(m)) if m.contains_key(key))
}

/// Clé de reprise :
/// sha1(f"{len(audio)} {sr} {LANG} {PAD} {GAP} {CHUNK} {segs} {MODEL}") (segs : liste de tuples).
fn key(len: usize, sr: u32, lang: &str, segs: &[(f64, f64)]) -> String {
    let list: Vec<String> = segs.iter().map(|(a, b)| format!("({}, {})", float_repr(*a), float_repr(*b))).collect();
    let text = format!("{len} {sr} {lang} {} {} {} [{}] {MODEL}", float_repr(PAD), float_repr(GAP), float_repr(CHUNK), list.join(", "));
    format!("{}\n", hex::encode(Sha1::digest(text.as_bytes())))
}

impl Ctx {
    /// Un seul Whisper pour les fichiers pas encore transcrits (modèle chargé une fois) ; JSON
    /// complet à côté de chaque fichier (<fichier>.json), avec le temps DTW de chaque jeton.
    /// Avancement : weights[fichier] atteint quand son JSON est écrit (temps du rush pour les
    /// morceaux, dans l'ordre ; nombre de segments pour les segments seuls).
    /// ctx : fenêtre de son réduite (-ac, 512 = 10 s : 3 fois plus rapide sur un segment court).
    fn whisper(&self, files: &[PathBuf], label: &str, weights: &HashMap<String, f64>, total_weight: f64, mut done: f64, ctx: u32) -> Result<f64, String> {
        let files: Vec<&PathBuf> = files.iter().filter(|f| !(self.resume && transcribed(f, "transcription"))).collect();
        if files.is_empty() {
            return Ok(done);
        }
        let model = paths::whisper_dir().join("ggml-large-v3-turbo-q8_0.bin");
        let mut cmd = Command::new(paths::whisper_bin("whisper-cli"));
        cmd.arg("-m").arg(&model).args(["-l", &self.lang, "-t", &threads().to_string(), "-mc", "0", "-ml", "1", "-sow", "-ojf", "-dtw", "large.v3.turbo"]);
        if ctx > 0 {
            cmd.args(["-ac", &ctx.to_string()]);
        }
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

    /// Parakeet sur du PCM 16 bits (modèle chargé une fois).
    fn recognize(&mut self, data: &[u8]) -> Result<Value, String> {
        if self.model.is_none() {
            self.model = Some(Parakeet::load()?);
        }
        let pcm: Vec<f32> = data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
        let r = self.model.as_mut().unwrap().recognize(&pcm)?;
        Ok(json!({"tokens": r.tokens, "timestamps": r.timestamps, "logprobs": r.logprobs}))
    }

    /// Parakeet sur les morceaux pas encore transcrits ; sortie à côté de chaque fichier
    /// (<fichier>.json, même forme que la version Python).
    fn parakeet(&mut self, files: &[PathBuf], label: &str, weights: &HashMap<String, f64>, total_weight: f64, mut done: f64) -> Result<f64, String> {
        for f in files {
            if self.resume && transcribed(f, "parakeet") {
                continue;
            }
            let (_, data) = read_wav(f)?;
            let out = json!({"parakeet": self.recognize(&data)?});
            let part = PathBuf::from(format!("{}.part", paths::display(&json_path(f))));
            fs::write(&part, out.to_string()).map_err(|e| format!("{} : {e}", paths::display(&part)))?;
            fs::rename(&part, json_path(f)).map_err(|e| e.to_string())?;
            if let Some(w) = weights.get(&paths::display(f)) {
                done = done.max(*w);
                report_every(label, done, total_weight, 0.0);
            }
        }
        Ok(done)
    }

    /// Range chaque mot dans le segment qui contient son début (le plus proche sinon) et
    /// convertit ses temps en temps du rush, bornés au segment.
    fn place_words(&self, found: &[Found], places: &[Place]) -> BTreeMap<usize, Vec<Value>> {
        let mut by_seg: BTreeMap<usize, Vec<(String, f64, f64, f64)>> = BTreeMap::new();
        let mut order: Vec<usize> = vec![];
        for (text, a, b, p) in found {
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
            by_seg.entry(i).or_default().push((text.clone(), cs + a - pos, cs + b - pos, *p));
        }
        let mut out = BTreeMap::new();
        for (i, mut ws) in by_seg {
            let (s, e) = self.segs[i];
            place_in_segment(&mut ws, s, e);
            out.insert(i, ws.into_iter().map(|(t, a, b, p)| json!({"w": t, "a": round_to(a, 3), "b": round_to(b, 3), "p": round_to(p, 2)})).collect());
        }
        out
    }
}

/// Durée la plus courte d'un mot prononcé : 40 ms par lettre, de 0,08 à 0,25 s.
fn min_len(word: &str) -> f64 {
    (0.04 * word.chars().filter(|c| c.is_alphanumeric()).count() as f64).clamp(0.08, 0.25)
}

/// Temps des mots d'un segment (texte, début, fin, confiance), bornés au segment [s, e]. Parakeet
/// place le premier mot après un silence 0,2 à 0,45 s trop tôt, dans le silence (le second un
/// peu aussi) : ramenés au début du segment, ils s'y empilaient sans durée. Les mots placés avant
/// le début du segment, et le suivant, commencent donc chacun après la durée minimale du mot
/// précédent ; les autres gardent le temps du modèle.
fn place_in_segment(ws: &mut [(String, f64, f64, f64)], s: f64, e: f64) {
    let mut piled = true;
    for k in 0..ws.len() {
        let raw = ws[k].1;
        let earliest = match k {
            0 => s,
            _ if piled => ws[k - 1].1 + min_len(&ws[k - 1].0),
            _ => ws[k - 1].1,
        };
        piled = piled && raw < s;
        ws[k].1 = raw.max(earliest).min(e);
    }
    for k in 0..ws.len() {
        // Fin d'un mot : début du suivant ; pour le dernier, fin donnée par le modèle (au
        // moins 0,3 s), bornée au segment.
        ws[k].2 = if k + 1 < ws.len() { ws[k + 1].1 } else { e.min(ws[k].2.max(ws[k].1 + 0.3)) };
    }
}

/// Mots de la sortie de Whisper : (texte, temps DTW du début, fin), en secondes du fichier.
/// Mots de la sortie de Whisper : (texte, temps DTW du début, fin, confiance), en secondes du fichier.
fn words_of(path: &Path) -> Result<Vec<Found>, String> {
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
        let p = tokens.iter().map(|k| crate::util::fd(k, "p", 1.0)).fold(1.0, f64::min);
        out.push((text, a, crate::util::fd(&offsets, "to", 0.0) / 1000.0, p));
    }
    Ok(out)
}

/// Mots de la sortie de Parakeet d'un morceau (voir words_from).
fn parakeet_words(path: &Path) -> Result<Vec<Found>, String> {
    let data = crate::util::read_json(json_path(path)).ok_or_else(|| format!("Sortie de Parakeet illisible : {}.json", paths::display(path)))?;
    Ok(words_from(data.get("parakeet").unwrap_or(&Value::Null), 0.0))
}

/// Mots d'une sortie de Parakeet (un jeton qui commence par une espace ouvre un mot), à partir de
/// l'instant « after » : (texte, début, fin, confiance). Fin : temps du dernier jeton + 0,16 s
/// (Parakeet ne donne que les débuts ; place_words la remplace par le début du mot suivant).
fn words_from(r: &Value, after: f64) -> Vec<Found> {
    let num = |v: &Value| v.as_f64().unwrap_or(0.0);
    let (tokens, times, lps) = (crate::util::arr(r, "tokens"), crate::util::arr(r, "timestamps"), crate::util::arr(r, "logprobs"));
    let mut out: Vec<(String, f64, f64, f64)> = vec![];
    let mut skip = false;
    for ((tok, t), lp) in tokens.iter().zip(times).zip(lps) {
        let (tok, t, lp) = (tok.as_str().unwrap_or(""), num(t), num(lp));
        if tok.starts_with(' ') || (out.is_empty() && !skip) {
            skip = t < after; // mot commencé avant « after » : écarté avec ses jetons
            if !skip {
                out.push((tok.trim().to_string(), t, t + 0.16, lp));
            }
        } else if !skip {
            if let Some(w) = out.last_mut() {
                w.0.push_str(tok);
                w.2 = t + 0.16;
                w.3 = w.3.min(lp);
            }
        }
    }
    out.into_iter().filter(|w| !w.0.is_empty()).map(|(w, a, b, lp)| (w, a, b, lp.exp())).collect()
}

/// Mots outils de chaque langue : un segment de Parakeet qui en a plus d'une autre langue que de la
/// sienne (au moins 2) est passé dans cette autre langue, souvent traduit (« définir l'information
/// from unity of mesure. A new unity… »). « a », « on » sont dans les deux langues : écartés.
const FUNCTION_WORDS: [(&str, &str); 2] = [
    ("fr", "le la les de des du et est un une que qui nous il ils elle pour dans ce c'est pas sur vous je avec mais donc"),
    ("en", "the is are and of to we have that this with will be you it's there what from they our has was for not so"),
];

/// Mots d'un segment passés dans une autre langue que celle du rush, ou écrits dans un autre
/// alphabet (cyrillique…).
fn foreign<'a>(ws: impl IntoIterator<Item = &'a str>, lang: &str) -> bool {
    let ws: Vec<&str> = ws.into_iter().collect();
    let latin = |c: char| !c.is_alphabetic() || c.is_ascii_alphabetic() || ('\u{c0}'..='\u{24f}').contains(&c);
    if ws.iter().any(|w| !w.chars().all(latin)) {
        return true;
    }
    let norm: Vec<String> = ws.iter().map(|w| w.to_lowercase().replace('’', "'").chars().filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '_').collect()).collect();
    let count = |list: &str| norm.iter().filter(|n| list.split(' ').any(|x| x == n.as_str())).count();
    let own = FUNCTION_WORDS.iter().find(|(l, _)| *l == lang).map_or(0, |(_, v)| count(v));
    let other = FUNCTION_WORDS.iter().filter(|(l, _)| *l != lang).map(|(_, v)| count(v)).max().unwrap_or(0);
    other >= 2 && other > own
}

fn texts(ws: &[Value]) -> impl Iterator<Item = &str> {
    ws.iter().map(|w| w["w"].as_str().unwrap_or(""))
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
    let mut ctx = Ctx { audio, segs, seg_dir, resume, lang, model: None };
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
        if !(ctx.resume && transcribed(&path, "parakeet")) {
            write_wav(&path, sr, &data);
        }
        let last = ctx.segs[*ids.last().unwrap()].1;
        jobs.push((path, places, total.min(last + PAD)));
    }

    // Avancement : temps du rush atteint par les morceaux transcrits, sur la durée du rush.
    let done_before = |p: &PathBuf| ctx.resume && transcribed(p, "parakeet");
    let already = jobs.iter().filter(|(p, _, _)| done_before(p)).map(|j| j.2).fold(0.0, f64::max);
    if already > 0.0 {
        let n = jobs.iter().filter(|(p, _, _)| done_before(p)).count();
        progress::say(&format!("Reprise de la transcription : {n} morceau(x) sur {} déjà transcrits", jobs.len()));
    }
    report_every("Transcription", already, total, 0.0);
    let mut words: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
    if !jobs.is_empty() {
        let files: Vec<PathBuf> = jobs.iter().map(|j| j.0.clone()).collect();
        let weights: HashMap<String, f64> = jobs.iter().map(|j| (paths::display(&j.0), j.2)).collect();
        ctx.parakeet(&files, "Transcription", &weights, total, already)?;
        for (path, places, _) in &jobs {
            words.extend(ctx.place_words(&parakeet_words(path)?, places));
        }
    }
    report_every("Transcription", total, total, 0.0);

    // Segments passés dans une autre langue que celle du rush : Parakeet les refait seuls, précédés de
    // deux segments sûrs dans la langue du rush (sans mot douteux), qui l'y ramènent quand il avait
    // traduit (« des smartwatchs, des monts intelligents » → « des montres intelligentes »). Ceux qui
    // restent dans l'autre langue y sont vraiment (présentateur anglais d'un exposé en français), sauf
    // s'ils mêlent des mots accentués d'un rush en français (« définit l'information from unity of
    // mesure ») : Whisper les refait, forcé en français, avec les segments vides.
    let lang = ctx.lang.clone();
    let moved: Vec<usize> = words.iter().filter(|(_, ws)| !ws.is_empty() && foreign(texts(ws), &lang)).map(|(i, _)| *i).collect();
    let mut mixed: Vec<usize> = vec![];
    if !moved.is_empty() {
        let sure: Vec<usize> = words
            .iter()
            .filter(|(i, ws)| ws.len() >= 5 && !moved.contains(i) && ws.iter().all(|w| crate::util::fd(w, "p", 1.0) >= DOUBT))
            .filter(|(i, _)| ctx.segs[**i].1 - ctx.segs[**i].0 <= 8.0)
            .map(|(i, _)| *i)
            .collect();
        let mut fixed = 0;
        for &i in &moved {
            let mut near = sure.clone();
            near.sort_by_key(|j| j.abs_diff(i));
            near.truncate(2);
            if near.is_empty() {
                break;
            }
            near.sort();
            let (s, e) = ctx.segs[i];
            let (cs, ce) = ((s - PAD).max(0.0), total.min(e + PAD));
            let mut data: Vec<u8> = vec![];
            for &j in &near {
                let (js, je) = ctx.segs[j];
                data.extend_from_slice(slice(&ctx.audio, at((js - PAD).max(0.0)), at(total.min(je + PAD))));
                data.extend(std::iter::repeat_n(0u8, 2 * (GAP * srf) as usize));
            }
            let pos = data.len() as f64 / 2.0 / srf;
            data.extend_from_slice(slice(&ctx.audio, at(cs), at(ce)));
            let found = words_from(&ctx.recognize(&data)?, pos - 0.1);
            if !found.is_empty() && !foreign(found.iter().map(|w| w.0.as_str()), &lang) {
                words.extend(ctx.place_words(&found, &[(pos, pos + ce - cs, cs, i)]));
                fixed += 1;
            }
        }
        let accent = Regex::new("[éèêàâçùûîôœ]").unwrap();
        mixed = moved
            .iter()
            .copied()
            .filter(|i| lang == "fr" && foreign(texts(&words[i]), &lang) && texts(&words[i]).any(|w| accent.is_match(&w.to_lowercase())))
            .collect();
        let list: String = moved
            .iter()
            .map(|i| format!("\n  #{i} « {} »", texts(&words[i]).collect::<Vec<_>>().join(" ").chars().take(70).collect::<String>()))
            .collect();
        progress::say(&format!(
            "{} segment(s) dans une autre langue que celle du rush, {fixed} ramené(s) dans sa langue, {} mêlé(s) refait(s) par Whisper{list}",
            moved.len(),
            mixed.len()
        ));
    }

    // Segments restés vides (hésitation isolée « Euh… », bout de phrase) et segments mêlés : transcrits
    // seuls par Whisper, forcé dans la langue du rush ; fenêtre de son réduite pour les segments courts
    // (3 fois plus vite). Whisper sans mot sur un segment mêlé : texte de Parakeet gardé.
    let mut empty: Vec<usize> = (0..ctx.segs.len()).filter(|i| words.get(i).is_none_or(|w| w.is_empty())).chain(mixed).collect();
    empty.sort();
    if !empty.is_empty() {
        progress::say(&format!("{} segment(s) transcrits seuls par Whisper", empty.len()));
        let mut alone: Vec<(PathBuf, Vec<Place>, f64, bool)> = vec![];
        for (k, &i) in empty.iter().enumerate() {
            let (s, e) = ctx.segs[i];
            let (cs, ce) = ((s - PAD).max(0.0), total.min(e + PAD));
            let path = ctx.seg_dir.join(format!("s{i:03}.wav"));
            if !(ctx.resume && transcribed(&path, "transcription")) {
                write_wav(&path, sr, slice(&ctx.audio, at(cs), at(ce)));
            }
            alone.push((path, vec![(0.0, ce - cs, cs, i)], (k + 1) as f64, ce - cs <= 9.0));
        }
        report_every("Segments transcrits seuls", 0.0, alone.len() as f64, 0.0);
        let weights: HashMap<String, f64> = alone.iter().map(|j| (paths::display(&j.0), j.2)).collect();
        let pick = |short: bool| -> Vec<PathBuf> { alone.iter().filter(|j| j.3 == short).map(|j| j.0.clone()).collect() };
        let done = ctx.whisper(&pick(true), "Segments transcrits seuls", &weights, alone.len() as f64, 0.0, 512)?;
        ctx.whisper(&pick(false), "Segments transcrits seuls", &weights, alone.len() as f64, done, 0)?;
        for (path, places, _, _) in &alone {
            words.extend(ctx.place_words(&words_of(path)?, places).into_iter().filter(|(_, ws)| !ws.is_empty()));
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
    let code = match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    };
    // ONNX Runtime (Parakeet) sur macOS : ses destructeurs de fin de programme s'arrêtent sur
    // « mutex lock failed: Invalid argument » (SIGABRT, code 134) alors que tout est écrit ;
    // sortie sans eux.
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        unsafe { libc::_exit(code) }
    }
    #[allow(unreachable_code)]
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_words() {
        assert!(foreign("définit l'information from unity of mesure. A new unity of mesure does".split(' '), "fr"));
        assert!(foreign("So you guys have been so amazing.".split(' '), "fr"));
        assert!(foreign(["де", "смарт"], "fr"));
        assert!(!foreign("on a besoin juste de quelques photos, par exemple".split(' '), "fr"));
        assert!(!foreign("a collecté, trié,".split(' '), "fr"));
        assert!(foreign("donc on a besoin de quelques photos pour le gâteau".split(' '), "en"));
    }

    #[test]
    fn words_after() {
        let r = json!({"tokens": [" Don", "c", " la", " com", "m", "una", "ut", "é"], "timestamps": [0.96, 1.04, 1.28, 1.52, 1.6, 1.68, 1.84, 2.0],
            "logprobs": [0.0, -0.1, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0]});
        let w = words_from(&r, 0.0);
        assert_eq!(w.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["Donc", "la", "communauté"]);
        assert!((w[2].3 - (-1f64).exp()).abs() < 1e-9);
        assert_eq!(words_from(&r, 1.2).iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["la", "communauté"]);
    }

    #[test]
    fn first_words_spread() {
        // « So you guys have » : Parakeet donne 11.53, 11.85, 12.09, 12.41 pour un segment qui
        // commence à 11.9 (début de la voix mesuré à 11.9, « you » vers 12.1, « guys » vers 12.25).
        let mut ws: Vec<(String, f64, f64, f64)> =
            [("So", 11.53), ("you", 11.85), ("guys", 12.09), ("have", 12.41)].iter().map(|(w, a)| (w.to_string(), *a, a + 0.16, 1.0)).collect();
        place_in_segment(&mut ws, 11.9, 13.8);
        let starts: Vec<f64> = ws.iter().map(|w| (w.1 * 100.0).round() / 100.0).collect();
        assert_eq!(starts, [11.9, 11.98, 12.1, 12.41]);
        assert!(ws.iter().all(|w| w.2 > w.1));
        // Mots déjà espacés : temps du modèle gardés.
        let mut ws = vec![("Bonjour".to_string(), 1.0, 1.2, 1.0), ("à".to_string(), 1.5, 1.6, 1.0)];
        place_in_segment(&mut ws, 0.9, 3.0);
        assert_eq!((ws[0].1, ws[1].1), (1.0, 1.5));
    }

    #[test]
    fn key_like_python() {
        // hashlib.sha1(f"{1000} {16000} fr {0.15} {1.0} {28.0} {[(0.0, 4.04), (5.56, 9.16)]} {MODEL}".encode()).hexdigest()
        let k = key(1000, 16000, "fr", &[(0.0, 4.04), (5.56, 9.16)]);
        let text = "1000 16000 fr 0.15 1.0 28.0 [(0.0, 4.04), (5.56, 9.16)] parakeet-tdt-0.6b-v3";
        assert_eq!(k, format!("{}\n", hex::encode(Sha1::digest(text.as_bytes()))));
    }
}
