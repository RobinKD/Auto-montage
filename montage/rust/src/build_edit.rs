//! Construit le montage du rush en cours à partir de la transcription.
//!
//! Entrées : work/segments.json (transcription par segment de parole),
//!           work/rush.txt (chemin du rush, écrit par prepare) pour l'audio d'origine 48 kHz.
//! Sorties : src/data/edit.json  (clips, sous-titres, zooms, effets, bruitages)
//!           public/audio/voice.wav (voix montée, débruitée, avec les voix modifiées faites par ffmpeg ;
//!                                   celles de HF_VOICES, les graves, la clarté et le limiteur sont
//!                                   ajoutés au rendu par HyperFrames : selection/hf_audio.js)
//!
//! Les décisions éditoriales (KEEP / FIXES / FUNNY / CASH / CHIPS / INTRO_TEXT) viennent de
//! work/edit_choices.json et désignent des segments de work/segments.json.
//!
//! Choix écrits pour le rush en cours dans work/edit_choices.json (CHOICES_PATH, par Claude,
//! d'après les consignes de montage) : ils remplacent le montage générique et survivent aux mises
//! à jour.
//!   {"rush": "MonRush.mov", "keep": [[segment, premier mot, dernier mot ou null], …],
//!    "fixes": [[segment, mot, "texte"], …], "funny": [[segment, premier, dernier, "voix"?], …],
//!            (voix modifiée : moments_lib::VOICES, « tremblement » par défaut, ou robot, ecureuil,
//!            lutin, geant, megaphone, telephone, salle, batterie, radio, choeur)
//!    "cash": [[segment, mot], …], "chips": [[segment, mot, "texte"], …], "intro": "texte",
//!    "sfx": [[segment, mot, "<son>"], …],  (sons par défaut de moments_lib::SOUNDS : whoosh, pop,
//!            ding, boum, montee, glitch, photo, faux, juste… ou personnels : "perso-<nom>")
//!    "vfx": {"segment": [effet visuel, …]}}  (comme sur la page, temps depuis le début de
//!            l'extrait du moment : {"kind": "zoomin"|"zoomsec"|"dezoom"|"shake", "at", "dur",
//!            "force" (%)}, {"kind": "flash", "at", "dur"}, {"kind": "highlight", "text": "mot",
//!            "color": "#rrggbb"}, {"kind": "zoomlibre", "at", "dur", "from": {"s", "x", "y"},
//!            "to": {"s", "x", "y"}} (zoom progressif d'un cadre à l'autre : grossissement s de 1 à 4,
//!            centre x, y de l'image visible de 0 à 1))

use crate::difflib::SequenceMatcher;
use crate::fonts_lib;
use crate::moments_lib::{
    app_mode, audio_map, batterie_warp, derush_subtitles, effective_gaps, frame_size, is_gap, is_hf_voice, load_segments,
    rush_duration, suggested_keep, voice_filter, voice_ids, word_map, SOUNDS,
};
use crate::paths;
use crate::pyjson;
use crate::util::{self, round_int, round_to, truthy};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

const FPS: f64 = 30.0;
const SR: usize = 48000;

// --- Petits outils : valeurs JSON lues comme Python les lirait ----------------------------

/// Dictionnaire Python à clés quelconques : ordre d'insertion, une clé déjà là garde sa place.
#[derive(Clone, Debug)]
struct OMap<K, V>(Vec<(K, V)>);

impl<K: PartialEq + Clone, V> OMap<K, V> {
    fn new() -> Self {
        OMap(vec![])
    }
    fn get(&self, k: &K) -> Option<&V> {
        self.0.iter().find(|(x, _)| x == k).map(|(_, v)| v)
    }
    fn contains(&self, k: &K) -> bool {
        self.get(k).is_some()
    }
    fn insert(&mut self, k: K, v: V) {
        match self.0.iter_mut().find(|(x, _)| *x == k) {
            Some(e) => e.1 = v,
            None => self.0.push((k, v)),
        }
    }
    /// `setdefault(k, default)`.
    fn entry(&mut self, k: K, default: impl FnOnce() -> V) -> &mut V {
        let i = match self.0.iter().position(|(x, _)| *x == k) {
            Some(i) => i,
            None => {
                self.0.push((k, default()));
                self.0.len() - 1
            }
        };
        &mut self.0[i].1
    }
    fn iter(&self) -> impl Iterator<Item = &(K, V)> {
        self.0.iter()
    }
    fn len(&self) -> usize {
        self.0.len()
    }
}

/// `float(x)` d'une valeur JSON ; `default` si elle manque (ou ne se lit pas comme un nombre).
fn pyfloat(v: Option<&Value>, default: f64) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(default),
        Some(Value::Bool(b)) => *b as i64 as f64,
        Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
        _ => default,
    }
}

/// `int(x)` d'une valeur JSON (flottant tronqué, texte d'un entier).
fn pyint(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        Value::Bool(b) => Some(*b as i64),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `str(x)` d'une valeur JSON.
fn pystr(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => if *b { "True" } else { "False" }.into(),
        Some(v) => pyjson::dumps_utf8(v),
    }
}

fn fnum(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

fn inum(v: &Value, k: &str) -> i64 {
    pyint(v.get(k)).unwrap_or(0)
}

fn num(x: f64) -> Value {
    util::num(x)
}

fn r3(x: f64) -> Value {
    num(round_to(x, 3))
}

/// Somme d'un tableau float32 comme numpy (sommation par paires, blocs de 128 dépliés par 8) :
/// les énergies de `refine` sont celles de `np.mean`, à l'identique, pour tomber sur la même
/// trame en cas de quasi-égalité.
fn pairwise_sum(a: &[f32]) -> f32 {
    let n = a.len();
    if n < 8 {
        let mut res = 0f32;
        for &x in a {
            res += x;
        }
        res
    } else if n <= 128 {
        let mut r = [0f32; 8];
        r.copy_from_slice(&a[..8]);
        let mut i = 8;
        while i < n - (n % 8) {
            for j in 0..8 {
                r[j] += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise_sum(&a[..n2]) + pairwise_sum(&a[n2..])
    }
}

/// `np.linspace(start, stop, n)` (pas multiplié puis début ajouté, dernière valeur exacte).
fn linspace(start: f64, stop: f64, n: usize) -> Vec<f64> {
    if n == 0 {
        return vec![];
    }
    if n == 1 {
        return vec![start];
    }
    let step = (stop - start) / (n - 1) as f64;
    let mut v: Vec<f64> = (0..n).map(|k| k as f64 * step + start).collect();
    v[n - 1] = stop;
    v
}

/// Échantillons int16 d'un signal (`np.clip(x, -1, 1) * 32767` puis `astype(np.int16)`).
fn to_i16_f32(x: &[f32]) -> Vec<i16> {
    x.iter().map(|&v| (v.clamp(-1.0, 1.0) * 32767f32) as i16).collect()
}

fn to_i16_f64(x: &[f64]) -> Vec<i16> {
    x.iter().map(|&v| (v.clamp(-1.0, 1.0) * 32767.0) as i16).collect()
}

fn write_wav(path: &Path, samples: &[i16]) -> Result<(), String> {
    let spec = hound::WavSpec { channels: 1, sample_rate: SR as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let err = |e: hound::Error| format!("{} : {e}", path.display());
    let mut w = hound::WavWriter::create(path, spec).map_err(err)?;
    for &s in samples {
        w.write_sample(s).map_err(err)?;
    }
    w.finalize().map_err(err)
}

fn read_wav(path: &Path) -> Result<Vec<f32>, String> {
    let r = hound::WavReader::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
    r.into_samples::<i16>()
        .map(|s| s.map(|x| x as f32 / 32768f32))
        .collect::<Result<Vec<f32>, _>>()
        .map_err(|e| format!("{} : {e}", path.display()))
}

/// ffmpeg qui doit réussir (`subprocess.run(…, check=True)`).
fn ffmpeg(args: &[String]) -> Result<(), String> {
    let st = Command::new(paths::ffmpeg()).args(args).stdin(Stdio::null()).status().map_err(|e| format!("ffmpeg : {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg a échoué (code {}) : {}", crate::procs::py_code(&st), args.join(" ")))
    }
}

// --- Moments ---------------------------------------------------------------------------

/// Moment gardé : (segment, premier mot, dernier mot ou None) pour un moment de parole, (plan,
/// début, fin en s depuis le début du plan) pour un plan sans parole, comme les tuples de KEEP.
#[derive(Clone, Debug)]
struct Keep {
    seg: i64,
    first: f64,
    last: Option<f64>,
}

struct Ctx {
    seg: HashMap<i64, Value>,
    gaps: HashMap<i64, (f64, f64)>,
}

impl Ctx {
    fn words(&self, id: i64) -> &[Value] {
        self.seg.get(&id).map(|s| util::arr(s, "words")).unwrap_or(&[])
    }
    fn start(&self, id: i64) -> f64 {
        self.seg.get(&id).map(|s| fnum(s, "start")).unwrap_or(0.0)
    }
    /// Début de l'extrait d'un moment sur la page (les temps de la page en partent).
    fn page_start(&self, id: i64) -> f64 {
        if is_gap(id) {
            // l'extrait d'un plan sans parole commence au début du plan
            return self.gaps.get(&id).map(|g| g.0).unwrap_or(0.0);
        }
        (self.start(id) - 0.05).max(0.0)
    }
    /// Fenêtre de voix modifiée d'un moment drôle, en temps de la page.
    fn funny_default(&self, id: i64, a: i64, b: i64, voice: Option<&str>) -> Value {
        let words = self.words(id);
        let (wa, wb) = (&words[a as usize], &words[b as usize]);
        let at = fnum(wa, "a") - 0.05 - self.page_start(id);
        let mut out = Map::new();
        out.insert("kind".into(), json!("voix"));
        out.insert("at".into(), num(round_to(at.max(0.0), 2)));
        out.insert("dur".into(), num(round_to(fnum(wb, "b") - fnum(wa, "a") + 0.15, 2)));
        let voice = voice.unwrap_or("tremblement");
        if voice != "tremblement" {
            out.insert("voice".into(), json!(voice));
        }
        Value::Object(out)
    }
}

#[derive(Clone, Debug)]
struct Clip {
    seg: i64,
    trim_before: i64,
    dur: i64,
    from: i64,
    text: String,
    trim_before_hd: Option<i64>,
}

impl Clip {
    fn end(&self) -> f64 {
        (self.from + self.dur) as f64 / FPS
    }
    fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("seg".into(), json!(self.seg));
        m.insert("trimBefore".into(), json!(self.trim_before));
        m.insert("durationInFrames".into(), json!(self.dur));
        m.insert("from".into(), json!(self.from));
        m.insert("text".into(), json!(self.text));
        if let Some(h) = self.trim_before_hd {
            m.insert("trimBeforeHd".into(), json!(h));
        }
        Value::Object(m)
    }
}

struct WordEvent {
    seg: i64,
    i: i64,
    text: String,
    start: f64,
    end: f64,
}

struct Caption {
    top: String,
    bottom: String,
    words: Vec<String>,
    times: Vec<f64>,
    start: f64,
    end: f64,
}

struct Funny {
    start: f64,
    end: f64,
    force: f64,
    voice: String,
}

struct Sfx {
    src: String,
    start: f64,
    volume: f64,
    dur: Option<f64>,
}

static NOT_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w]").unwrap());

/// Textes corrigés sur la page : les mots corrigés reprennent les horodatages des mots
/// d'origine (alignement mot à mot).
fn realign(words: &[Value], corrected: &str) -> Vec<String> {
    let norm = |w: &str| NOT_WORD.replace_all(&w.to_lowercase(), "").into_owned();
    let old: Vec<String> = words.iter().map(|w| w.get("w").and_then(Value::as_str).unwrap_or("").to_string()).collect();
    let new: Vec<&str> = corrected.split_whitespace().collect();
    let mut texts = vec![String::new(); old.len()];
    let a: Vec<String> = old.iter().map(|w| norm(w)).collect();
    let b: Vec<String> = new.iter().map(|w| norm(w)).collect();
    let ops = SequenceMatcher::new(&a, &b).get_opcodes();
    for (_, i1, i2, j1, j2) in ops {
        if i1 == i2 {
            // mots ajoutés : collés au mot précédent (ou au suivant en tête)
            if texts.is_empty() {
                continue;
            }
            let k = if i1 > 0 { i1 - 1 } else { 0 };
            let extra = new[j1..j2].join(" ");
            texts[k] = if i1 > 0 {
                format!("{} {}", texts[k], extra).trim().to_string()
            } else {
                format!("{} {}", extra, texts[k]).trim().to_string()
            };
            continue;
        }
        let chunk = &new[j1..j2];
        for (n, w) in chunk.iter().enumerate() {
            // répartis sur la durée des mots d'origine
            let k = i1 + n * (i2 - i1) / chunk.len();
            texts[k] = format!("{} {}", texts[k], w).trim().to_string();
        }
    }
    texts
}

/// Coupure en 2 lignes de longueurs les plus proches (en lettres) ; 0 pour un seul mot.
fn balanced_split(words: &[String]) -> usize {
    if words.len() < 2 {
        return 0;
    }
    let lens: Vec<i64> = words.iter().map(|w| w.chars().count() as i64).collect();
    let total: i64 = lens.iter().sum::<i64>() + words.len() as i64 - 1;
    let (mut best, mut best_k) = (None::<i64>, 1);
    for k in 1..words.len() {
        let top: i64 = lens[..k].iter().sum::<i64>() + k as i64 - 1;
        let diff = (top - (total - top - 1)).abs();
        if best.map(|b| diff < b).unwrap_or(true) {
            best = Some(diff);
            best_k = k;
        }
    }
    best_k
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

fn run() -> Result<(), String> {
    let root = paths::root();
    let work = paths::work();
    let fps_i = FPS as i64;

    // Moments de parole : la transcription, avec les scissions et fusions faites sur la page
    // (work/decoupage.json). SEG : id -> moment ; les choix ci-dessous (KEEP, FIXES…) désignent
    // les segments d'origine et leurs mots, convertis plus bas (word_map).
    let (raw_segments, segments, deco) = load_segments(&work)?;
    let wmap = word_map(&segments);
    // Plans sans parole (entre les segments), ajoutables depuis la page : moments_lib::is_gap.
    let gaps: HashMap<i64, (f64, f64)> = effective_gaps(&raw_segments, &segments, rush_duration(&work))
        .iter()
        .map(|g| (inum(g, "id"), (fnum(g, "start"), fnum(g, "end"))))
        .collect();
    let mut ctx = Ctx { seg: segments.iter().map(|s| (inum(s, "id"), s.clone())).collect(), gaps };

    // --- Dérush -----------------------------------------------------------------
    // Choix de montage du rush en cours, lus dans work/edit_choices.json (format en tête du fichier).
    // KEEP : (segment, premier mot gardé, dernier mot gardé), None = jusqu'au bout.
    // FIXES : corrections de transcription pour les sous-titres, (segment, index du mot) -> texte.
    // FUNNY : moments « drôles » à voix modifiée (segment, premier mot, dernier mot[, voix]),
    //         proposés sur la page comme effet « Voix modifiée » ; l'effet visuel sur le visage
    //         est fait à la main au montage final.
    // CASH : bruit de caisse enregistreuse sur un mot (segment, mot).
    // CHIPS : éléments en surimpression avec son « bop » (segment, mot, texte).
    // EXTRA_SFX : sons placés sur un mot (segment, mot, "<son>" ou "perso-<nom>").
    // EXTRA_VFX : effets visuels placés par Claude, {segment: [effets]}.
    // INTRO_TEXT : intro tapée au clavier, pendant la première phrase.
    let mut keep: Vec<Keep> = vec![];
    let mut fixes: OMap<(i64, i64), String> = OMap::new();
    let mut funny_c: Vec<(i64, i64, i64, Option<String>)> = vec![];
    let mut cash: Vec<(i64, i64)> = vec![];
    let mut chips_c: Vec<(i64, i64, String)> = vec![];
    let mut extra_sfx: Vec<(i64, i64, String)> = vec![];
    let mut extra_vfx: OMap<i64, Vec<Value>> = OMap::new();
    let mut intro_text = String::new();

    // Sans edit_choices.json pour ce rush (préparé depuis l'interface, sans Claude) : montage
    // générique, moments suggérés, sans effets ni intro.
    let mut edit_rush: Option<String> = None;
    let rush_txt = fs::read_to_string(work.join("rush.txt")).map_err(|e| format!("{} : {e}", work.join("rush.txt").display()))?;
    let rush_rel = rush_txt.trim().to_string();
    let current_rush = Path::new(&rush_rel).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

    let choices_path = work.join("edit_choices.json");
    if choices_path.exists() {
        let c = util::read_json(&choices_path).ok_or_else(|| format!("{} illisible", choices_path.display()))?;
        if c.get("rush").and_then(Value::as_str) == Some(current_rush.as_str()) {
            edit_rush = Some(current_rush.clone());
            let ints = |x: &Value, n: usize| -> Vec<i64> { (0..n).map(|k| pyint(x.get(k)).unwrap_or(0)).collect() };
            if truthy(c.get("keep")) {
                keep = util::arr(&c, "keep")
                    .iter()
                    .map(|k| Keep {
                        seg: pyint(k.get(0)).unwrap_or(0),
                        first: pyint(k.get(1)).unwrap_or(0) as f64,
                        last: match k.get(2) {
                            None | Some(Value::Null) => None,
                            v => Some(pyint(v).unwrap_or(0) as f64),
                        },
                    })
                    .collect();
            }
            for f in util::arr(&c, "fixes") {
                let k = ints(f, 2);
                fixes.insert((k[0], k[1]), pystr(f.get(2)));
            }
            let ids = voice_ids();
            for f in util::arr(&c, "funny") {
                let k = ints(f, 3);
                let voice = f.get(3).map(|v| pystr(Some(v))).filter(|v| ids.contains(&v.as_str()));
                funny_c.push((k[0], k[1], k[2], voice));
            }
            for x in util::arr(&c, "cash") {
                let k = ints(x, 2);
                cash.push((k[0], k[1]));
            }
            for x in util::arr(&c, "chips") {
                let k = ints(x, 2);
                chips_c.push((k[0], k[1], pystr(x.get(2))));
            }
            for x in util::arr(&c, "sfx") {
                let k = ints(x, 2);
                extra_sfx.push((k[0], k[1], pystr(x.get(2))));
            }
            if let Some(Value::Object(v)) = c.get("vfx") {
                for (k, vs) in v {
                    let Ok(k) = k.trim().parse::<i64>() else { continue };
                    let list = vs.as_array().map(|a| a.iter().filter(|x| x.is_object()).cloned().collect()).unwrap_or_default();
                    extra_vfx.insert(k, list);
                }
            }
            intro_text = if truthy(c.get("intro")) { pystr(c.get("intro")) } else { String::new() };
            if !truthy(c.get("keep")) {
                // sans liste : les moments suggérés
                let (ids, _) = suggested_keep(&segments, &work);
                let mut ids: Vec<i64> = ids.into_iter().collect();
                ids.sort();
                keep = ids.into_iter().map(|sid| Keep { seg: sid, first: 0.0, last: None }).collect();
            }
        }
    }
    let same_rush = edit_rush.as_deref() == Some(current_rush.as_str());
    if same_rush && (!deco.splits.is_empty() || !deco.merges.is_empty()) {
        // Choix écrits pour les segments d'origine : vers les moments scindés ou fusionnés.
        let at = |seg: i64, i: i64| wmap.get(&(seg, i)).copied();
        let mut new_keep = vec![];
        for k in &keep {
            let (seg, first) = (k.seg, k.first as i64);
            let n = if 0 <= seg && (seg as usize) < raw_segments.len() { util::arr(&raw_segments[seg as usize], "words").len() as i64 } else { 0 };
            let last = k.last.map(|l| l as i64).unwrap_or(n - 1);
            let mut spans: OMap<i64, Vec<i64>> = OMap::new();
            for i in first.max(0)..n.min(last + 1) {
                if let Some((eid, ix)) = at(seg, i) {
                    spans.entry(eid, Vec::new).push(ix);
                }
            }
            for (eid, ix) in spans.iter() {
                let (lo, hi) = (*ix.iter().min().unwrap(), *ix.iter().max().unwrap());
                let n_eff = ctx.words(*eid).len() as i64;
                new_keep.push(Keep { seg: *eid, first: lo as f64, last: if hi == n_eff - 1 { None } else { Some(hi as f64) } });
            }
        }
        keep = new_keep;
        let mut nf = OMap::new();
        for ((s, i), v) in fixes.iter() {
            if let Some(k) = at(*s, *i) {
                nf.insert(k, v.clone());
            }
        }
        fixes = nf;
        funny_c = funny_c
            .iter()
            .filter_map(|(s, a, b, v)| match (at(*s, *a), at(*s, *b)) {
                (Some(x), Some(y)) if x.0 == y.0 => Some((x.0, x.1, y.1, v.clone())),
                _ => None,
            })
            .collect();
        cash = cash.iter().filter_map(|(s, i)| at(*s, *i)).collect();
        chips_c = chips_c.iter().filter_map(|(s, i, t)| at(*s, *i).map(|(e, j)| (e, j, t.clone()))).collect();
        extra_sfx = extra_sfx.iter().filter_map(|(s, i, t)| at(*s, *i).map(|(e, j)| (e, j, t.clone()))).collect();
        // Effets visuels de Claude (temps depuis le début de l'extrait du segment) : sur le moment
        // qui contient le 1er mot du segment, décalés du début de ce moment.
        let mut vfx: OMap<i64, Vec<Value>> = OMap::new();
        for (seg, fxs) in extra_vfx.iter() {
            if let Some((eid, _)) = at(*seg, 0) {
                let shift = raw_segments.get(*seg as usize).map(|r| fnum(r, "start")).unwrap_or(0.0) - ctx.start(eid);
                let list = vfx.entry(eid, Vec::new);
                for v in fxs {
                    if v.get("at").is_some() {
                        let mut o = v.as_object().cloned().unwrap_or_default();
                        o.insert("at".into(), r3(pyfloat(v.get("at"), 0.0) + shift));
                        list.push(Value::Object(o));
                    } else {
                        list.push(v.clone());
                    }
                }
            }
        }
        extra_vfx = vfx;
    }
    if !same_rush {
        if segments.is_empty() {
            return Err("Aucune parole détectée dans le rush : rien à monter.".into());
        }
        let (keep_ids, _) = suggested_keep(&segments, &work);
        keep = segments
            .iter()
            .map(|s| inum(s, "id"))
            .filter(|id| keep_ids.contains(id))
            .map(|id| Keep { seg: id, first: 0.0, last: None })
            .collect();
        if keep.is_empty() {
            keep = segments.iter().map(|s| Keep { seg: inum(s, "id"), first: 0.0, last: None }).collect();
        }
        fixes = OMap::new();
        funny_c.clear();
        cash.clear();
        chips_c.clear();
        intro_text.clear();
        extra_sfx.clear();
        extra_vfx = OMap::new();
    }

    // --- Sélection faite sur la page des moments ---------------------------------------
    // Si work/selection.json existe ({"keep": [ids de segments]}, écrit par Claude depuis la
    // page), elle remplace KEEP : segments entiers dans l'ordre du rush (découpes de la page
    // comprises, pas les coupes au mot de KEEP). Les effets qui visent un segment retiré sont ignorés.
    // Habillage par défaut de chaque moment, pour l'aperçu des moments sur la page (avant
    // que la sélection ne filtre ces listes).
    let mut overlay = Map::new();
    overlay.insert("fixes".into(), json!({}));
    let mut cash_word = Map::new();
    for (seg, i) in &cash {
        cash_word.insert(seg.to_string(), json!(i));
    }
    overlay.insert("cashWord".into(), Value::Object(cash_word));
    let mut chips_d = Map::new();
    for (seg, i, label) in &chips_c {
        chips_d.insert(seg.to_string(), json!({"word": i, "text": label}));
    }
    overlay.insert("chips".into(), Value::Object(chips_d));
    overlay.insert("intro".into(), json!(intro_text));
    overlay.insert("introAt".into(), json!(0.2)); // départ de l'intro dans l'extrait du 1er moment gardé
    overlay.insert("introType".into(), json!(1.6)); // durée de frappe
    let mut vfx_d = Map::new(); // effets visuels placés par Claude
    for (seg, v) in extra_vfx.iter() {
        vfx_d.insert(seg.to_string(), Value::Array(v.clone()));
    }
    overlay.insert("vfx".into(), Value::Object(vfx_d));
    {
        let fx = overlay.get_mut("fixes").unwrap().as_object_mut().unwrap();
        for ((seg, i), text) in fixes.iter() {
            let e = fx.entry(seg.to_string()).or_insert_with(|| json!({}));
            e.as_object_mut().unwrap().insert(i.to_string(), json!(text));
        }
    }
    let intro_at = 0.2;
    let intro_type = 1.6;

    // Effets sonores proposés par défaut sur la page, par moment (repris de CASH et FUNNY :
    // « voix » = voix modifiée, qui tremble).
    let mut sfx_defaults: OMap<i64, Vec<Value>> = OMap::new();
    for (seg, _) in &cash {
        sfx_defaults.entry(*seg, || vec![json!("cash")]);
    }
    for (seg, i, kind) in &extra_sfx {
        // sons personnels : sur leur mot, réglables sur la page
        let words = ctx.words(*seg);
        if 0 <= *i && (*i as usize) < words.len() {
            let at = fnum(&words[*i as usize], "a") - ctx.page_start(*seg);
            let v = json!({"kind": kind, "at": num(round_to(at, 3)), "repeat": 1, "gap": 0.1});
            sfx_defaults.entry(*seg, Vec::new).push(v);
        }
    }
    let mut voice_defaults: OMap<i64, Value> = OMap::new();
    for (seg, a, b, v) in &funny_c {
        voice_defaults.insert(*seg, ctx.funny_default(*seg, *a, *b, v.as_deref()));
    }
    for (seg, v) in voice_defaults.iter() {
        sfx_defaults.entry(*seg, Vec::new).push(v.clone());
    }
    // Découpes faites sur la page : segment -> instants de coupe (s depuis le début de l'extrait),
    // et parties gardées : segment -> {indices} (None = tout le segment).
    let mut cuts: HashMap<i64, Vec<f64>> = HashMap::new();
    let mut parts: BTreeMap<i64, Option<BTreeSet<i64>>> = BTreeMap::new();
    // Moments dont les effets ont été enregistrés avant l'effet « voix » : défaut de FUNNY gardé.
    let mut sfx_legacy: HashSet<i64> = HashSet::new();
    // Effets choisis sur la page : segment -> liste d'effets. Chaque effet est soit un nom
    // ("cash", "bop", "clavier" : réglages par défaut), soit un réglage précis
    // {"kind", "at": départ en s depuis le début de l'extrait du moment, "repeat", "gap", "dur"}.
    let mut page_sfx: OMap<i64, Vec<Value>> = OMap::new();
    // Effets visuels choisis sur la page : segment -> [{"kind": "typed" | "chip", "text", "at",
    // "type" (durée de frappe, texte tapé), "dur" (durée d'affichage), "sound": bool}].
    // Sans choix sur la page : intro tapée sur le 1er moment gardé, pastilles CHIPS.
    let mut page_vfx: OMap<i64, Vec<Value>> = OMap::new();
    let mut page_gains: HashMap<i64, f64> = HashMap::new(); // volume de la voix par moment (page des moments)
    let mut page_focus: HashMap<i64, i64> = HashMap::new(); // cadrage sur une personne par moment (detect_face)

    let selection_path = work.join("selection.json");
    let selection: Option<Value> = if selection_path.exists() {
        Some(util::read_json(&selection_path).ok_or_else(|| format!("{} illisible", selection_path.display()))?)
    } else {
        None
    };
    if let Some(selection) = &selection {
        // « keep » : ids de moments, ou « id/k » pour la partie k d'un moment découpé.
        for item in util::arr(selection, "keep") {
            match item.as_str().filter(|s| s.contains('/')) {
                Some(s) => {
                    let mut it = s.split('/').map(|x| x.trim().parse::<i64>());
                    let (Some(Ok(mid)), Some(Ok(k))) = (it.next(), it.next()) else { continue };
                    match parts.get_mut(&mid) {
                        Some(None) => {}
                        Some(Some(set)) => {
                            set.insert(k);
                        }
                        None => {
                            parts.insert(mid, Some(BTreeSet::from([k])));
                        }
                    }
                }
                None => {
                    if let Some(mid) = pyint(Some(item)) {
                        parts.insert(mid, None);
                    }
                }
            }
        }
        if let Some(Value::Object(c)) = selection.get("cuts") {
            for (k, v) in c {
                let Ok(k) = k.trim().parse::<i64>() else { continue };
                if truthy(Some(v)) && !is_gap(k) {
                    let mut list: Vec<f64> = v.as_array().map(|a| a.iter().map(|x| pyfloat(Some(x), 0.0)).collect()).unwrap_or_default();
                    list.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    cuts.insert(k, list);
                }
            }
        }
        sfx_legacy = util::arr(selection, "sfx_legacy").iter().filter_map(|x| pyint(Some(x))).collect();
        if let Some(Value::Object(g)) = selection.get("gains") {
            for (k, v) in g {
                if let Ok(k) = k.trim().parse::<i64>() {
                    page_gains.insert(k, pyfloat(Some(v), 1.0));
                }
            }
        }
        if let Some(Value::Object(g)) = selection.get("focus") {
            for (k, v) in g {
                if let (Ok(k), Some(v)) = (k.trim().parse::<i64>(), pyint(Some(v))) {
                    page_focus.insert(k, v);
                }
            }
        }
        let mut page_trims: HashMap<i64, Value> = HashMap::new();
        if let Some(Value::Object(t)) = selection.get("trims") {
            for (k, v) in t {
                if let Ok(k) = k.trim().parse::<i64>() {
                    page_trims.insert(k, v.clone());
                }
            }
        }
        keep = vec![];
        for &mid in parts.keys() {
            if !is_gap(mid) && !ctx.seg.contains_key(&mid) {
                continue; // moment qui n'existe plus (scindé ou fusionné depuis)
            }
            if !is_gap(mid) {
                // Moment entier, comme sur la page : les coupes au mot de KEEP ne valent que pour le
                // montage par défaut ; sur la page, on coupe avec « Couper ici ».
                keep.push(Keep { seg: mid, first: 0.0, last: None });
            } else if let Some(&(gs, ge)) = ctx.gaps.get(&mid) {
                // Plan sans parole : début et fin choisis sur la page, en s depuis le début du plan.
                let length = ge - gs;
                let t = page_trims.get(&mid).cloned().unwrap_or(json!({}));
                let t_in = pyfloat(t.get("in"), 0.0).max(0.0).min(length - 0.2);
                let t_out = pyfloat(t.get("out"), length).max(t_in + 0.2).min(length);
                keep.push(Keep { seg: mid, first: t_in, last: Some(t_out) });
            }
        }
        // Dans l'ordre du rush, plans sans parole compris.
        let key = |k: &Keep| if is_gap(k.seg) { ctx.gaps[&k.seg].0 + k.first } else { ctx.start(k.seg) };
        keep.sort_by(|a, b| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal));
        let kept_ids: HashSet<i64> = keep.iter().map(|k| k.seg).collect();
        funny_c.retain(|f| kept_ids.contains(&f.0));
        chips_c.retain(|c| kept_ids.contains(&c.0));

        // Textes corrigés sur la page : les mots corrigés reprennent les horodatages des mots
        // d'origine (alignement mot à mot), et remplacent les FIXES de ce segment.
        let corrections = selection.get("corrections").and_then(Value::as_object).cloned().unwrap_or_default();
        for (seg_key, text) in &corrections {
            let Ok(seg_id) = seg_key.trim().parse::<i64>() else { continue };
            if !kept_ids.contains(&seg_id) {
                continue;
            }
            let text = pystr(Some(text));
            let words = ctx.words(seg_id).to_vec();
            // Texte vide : sous-titres supprimés pour ce moment (mots gardés, sans texte affiché).
            let texts = if !text.trim().is_empty() { realign(&words, &text) } else { vec![String::new(); words.len()] };
            if let Some(Value::Array(ws)) = ctx.seg.get_mut(&seg_id).and_then(|s| s.get_mut("words")) {
                for (w, t) in ws.iter_mut().zip(texts) {
                    if let Some(o) = w.as_object_mut() {
                        o.insert("w".into(), json!(t));
                    }
                }
            }
            fixes.0.retain(|(k, _)| k.0 != seg_id);
        }

        let as_list = |v: &Value| v.as_array().cloned().unwrap_or_default();
        if let Some(Value::Object(s)) = selection.get("sfx") {
            for (k, v) in s {
                if let Ok(k) = k.trim().parse::<i64>() {
                    if kept_ids.contains(&k) {
                        page_sfx.insert(k, as_list(v));
                    }
                }
            }
        }
        if let Some(Value::Object(s)) = selection.get("vfx") {
            for (k, v) in s {
                if let Ok(k) = k.trim().parse::<i64>() {
                    if kept_ids.contains(&k) {
                        page_vfx.insert(k, as_list(v));
                    }
                }
            }
        }
        // Un moment dont les effets ont été choisis sur la page ne garde la caisse
        // (placée sur le bon mot) que si elle est toujours cochée.
        // La caisse par défaut reste sur son mot tant qu'elle est cochée sans réglage précis.
        cash.retain(|c| {
            kept_ids.contains(&c.0) && page_sfx.get(&c.0).map(|l| l.contains(&json!("cash"))).unwrap_or(true)
        });
        // Sons personnels de Claude : remplacés par les effets choisis sur la page pour ce moment.
        extra_sfx.retain(|x| kept_ids.contains(&x.0) && !page_sfx.contains(&x.0));
        println!(
            "Sélection de la page : {} segments, {} textes corrigés, {} moments avec effets choisis",
            keep.len(),
            corrections.len(),
            page_sfx.len()
        );
    }

    // --- Mode « Dérushage » (bandeau de l'interface locale, work/mode.txt) ----------------------
    // Seulement les moments gardés, coupés comme sur la page : ni effets, ni voix modifiée, ni
    // cadrage, ni réglages du son ; plus bas, ni zooms, et les sous-titres (corrigés sur la page)
    // seulement si la case « Sous-titres » est cochée (work/derush.json). Les choix d'effets restent
    // enregistrés et reviennent en mode Auto-montage.
    let derush = app_mode(&work) == "derush";
    let mut derush_subs = true;
    if derush {
        funny_c.clear();
        cash.clear();
        chips_c.clear();
        extra_sfx.clear();
        extra_vfx = OMap::new();
        intro_text.clear();
        voice_defaults = OMap::new();
        page_gains.clear();
        page_focus.clear();
        page_sfx = OMap::new();
        page_vfx = OMap::new();
        for k in &keep {
            page_sfx.insert(k.seg, vec![]);
            page_vfx.insert(k.seg, vec![]);
        }
        derush_subs = derush_subtitles(&work);
        println!("Mode Dérushage : moments coupés, sans effets{}", if derush_subs { "" } else { " ni sous-titres" });
    }

    // --- Audio d'origine ----------------------------------------------------------
    // Piste son du rush, ou toutes ses pistes mélangées (moments_lib::audio_map, comme prepare).
    let rush_path = root.join(&rush_rel);
    let mut args: Vec<String> = vec!["-v".into(), "error".into(), "-i".into(), paths::display(&rush_path)];
    args.extend(audio_map(&rush_path, "son").unwrap_or_else(|| vec!["-map".into(), "0:a:0".into()]));
    args.extend(["-ac", "1", "-ar", &SR.to_string(), "-f", "s16le", "-"].iter().map(|s| s.to_string()));
    let out = Command::new(paths::ffmpeg())
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("ffmpeg : {e}"))?;
    if !out.status.success() {
        return Err(format!("ffmpeg n'a pas pu lire le son du rush :\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let audio: Vec<f32> = out.stdout.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768f32).collect();

    // Recale une coupe au point le plus silencieux autour de t (trames de 10 ms).
    let refine = |t: f64| -> f64 {
        let window = 0.12;
        let hop = (SR as f64 * 0.01) as i64;
        let (lo, hi) = (((t - window) * SR as f64) as i64, ((t + window) * SR as f64) as i64);
        let (mut best, mut best_e) = (t, 1e9f64);
        let mut s = lo.max(0);
        let stop = (audio.len() as i64 - hop).min(hi);
        let mut sq = vec![0f32; hop as usize];
        while s < stop {
            for (k, x) in audio[s as usize..(s + hop) as usize].iter().enumerate() {
                sq[k] = x * x;
            }
            let e = (pairwise_sum(&sq) / hop as f32) as f64;
            if e < best_e {
                best = s as f64 / SR as f64;
                best_e = e;
            }
            s += hop;
        }
        best
    };

    // --- Clips --------------------------------------------------------------------
    let mut clips: Vec<Clip> = vec![];
    let mut out_frame: i64 = 0;
    let mut word_events: Vec<WordEvent> = vec![]; // mots avec leur temps de sortie
    let fixes_get = |fixes: &OMap<(i64, i64), String>, k: (i64, i64)| fixes.get(&k).cloned();
    for k in &keep {
        let seg_id = k.seg;
        if is_gap(seg_id) {
            // plan sans parole : pas de mots, pas de sous-titre
            let Some(&(gs, _)) = ctx.gaps.get(&seg_id) else { continue };
            let last = k.last.unwrap_or(0.0);
            let f_in = round_int((gs + k.first) * FPS);
            let dur = (round_int((gs + last) * FPS) - f_in).max(1);
            clips.push(Clip { seg: seg_id, trim_before: f_in, dur, from: out_frame, text: "Sans parole".into(), trim_before_hd: None });
            out_frame += dur;
            continue;
        }
        let Some(seg) = ctx.seg.get(&seg_id) else { continue };
        let words = util::arr(seg, "words");
        let first = k.first as i64;
        let last_i = match k.last {
            None => words.len() as i64 - 1,
            Some(l) => l as i64,
        };
        let start = if first == 0 { fnum(seg, "start") } else { refine(fnum(&words[first as usize], "a")) };
        let end = if k.last.is_none() { fnum(seg, "end") } else { refine(fnum(&words[last_i as usize], "b")) };
        // Découpe faite sur la page : parties [0, coupe 1], [coupe 1, coupe 2]… de l'extrait,
        // coupes à l'image près où elles ont été placées (curseur de la page, image par image) ;
        // parties gardées contiguës fusionnées.
        let mut spans: Vec<(f64, f64, Option<BTreeSet<usize>>)> = vec![(start, end, None)]; // (début, fin, parties ; None = tout)
        let mut bounds: Vec<f64> = vec![];
        if let Some(cs) = cuts.get(&seg_id) {
            bounds.push(-1e9);
            for c in cs {
                bounds.push(round_int((ctx.page_start(seg_id) + c) * FPS) as f64 / FPS);
            }
            bounds.push(1e9);
            let kept_parts = parts.get(&seg_id).cloned().flatten();
            spans.clear();
            for kk in 0..bounds.len() - 1 {
                let (a, b) = (start.max(bounds[kk]), end.min(bounds[kk + 1]));
                if b - a < 0.1 || kept_parts.as_ref().map(|p| !p.contains(&(kk as i64))).unwrap_or(false) {
                    continue;
                }
                match spans.last_mut() {
                    Some(last) if (last.1 - a).abs() < 1e-6 => {
                        last.1 = b;
                        last.2.get_or_insert_with(BTreeSet::new).insert(kk);
                    }
                    _ => spans.push((a, b, Some(BTreeSet::from([kk])))),
                }
            }
        }
        for (span_start, span_end, span_parts) in spans {
            let f_in = round_int(span_start * FPS);
            let f_out = round_int(span_end * FPS);
            let dur = f_out - f_in;
            clips.push(Clip { seg: seg_id, trim_before: f_in, dur, from: out_frame, text: String::new(), trim_before_hd: None });
            let n_before = word_events.len();
            for i in first..=last_i {
                let Some(w) = words.get(i as usize) else { continue };
                if let Some(sp) = &span_parts {
                    // moment découpé : mot rangé dans sa partie par son milieu
                    let mid = (fnum(w, "a") + fnum(w, "b")) / 2.0;
                    let part = (0..bounds.len() - 1).find(|&kk| mid < bounds[kk + 1]);
                    if !part.map(|p| sp.contains(&p)).unwrap_or(false) {
                        continue;
                    }
                }
                let text = fixes_get(&fixes, (seg_id, i)).unwrap_or_else(|| w.get("w").and_then(Value::as_str).unwrap_or("").to_string());
                let a = fnum(w, "a").max(f_in as f64 / FPS).min(f_out as f64 / FPS);
                let b = fnum(w, "b").max(a + 0.05).min(f_out as f64 / FPS);
                word_events.push(WordEvent {
                    seg: seg_id,
                    i,
                    text,
                    start: out_frame as f64 / FPS + (a - f_in as f64 / FPS),
                    end: out_frame as f64 / FPS + (b - f_in as f64 / FPS),
                });
            }
            let joined: Vec<&str> = word_events[n_before..].iter().filter(|x| !x.text.is_empty()).map(|x| x.text.as_str()).collect();
            clips.last_mut().unwrap().text = joined.join(" ");
            out_frame += dur;
        }
    }
    let total_frames = out_frame;
    let total = total_frames as f64 / FPS;

    // --- Source 4K du rendu final --------------------------------------------------------
    // Seules les images gardées (plus une marge) sont encodées en 4K par make_hd, bout à
    // bout : chaque clip y a donc sa propre position (trimBeforeHd).
    // Les plages qui se chevauchent sont fusionnées (le fichier 4K suit l'ordre du rush et
    // ne contient chaque image qu'une fois).
    const HD_PAD: i64 = 6;
    let mut ranges: Vec<(i64, i64)> = clips.iter().map(|c| ((c.trim_before - HD_PAD).max(0), c.trim_before + c.dur + HD_PAD)).collect();
    ranges.sort();
    let mut hd_ranges: Vec<[i64; 2]> = vec![];
    for (a, b) in ranges {
        match hd_ranges.last_mut() {
            Some(last) if a <= last[1] => last[1] = last[1].max(b),
            _ => hd_ranges.push([a, b]),
        }
    }
    for c in clips.iter_mut() {
        let mut offset = 0;
        for &[a, b] in &hd_ranges {
            if a <= c.trim_before && c.trim_before < b {
                c.trim_before_hd = Some(offset + c.trim_before - a);
                break;
            }
            offset += b - a;
        }
    }

    // Temps d'un mot dans le montage, None s'il a été coupé.
    let word_time = |seg_id: i64, i: i64| word_events.iter().find(|w| w.seg == seg_id && w.i == i).map(|w| w.start);

    // --- Sous-titres ----------------------------------------------------------------
    // Jusqu'à 6 mots (26 lettres) sur 2 lignes (3 mots sur 1 ligne, 9 sur 3 : nombre de lignes choisi
    // dans le style, fonts_lib::CAPTION_LIMITS), de largeurs aussi proches que possible compte tenu de
    // la taille de chaque ligne : les coupures sont choisies au rendu, en mesurant le texte dans la
    // police choisie (render_lib::caption_layout) ; « top » / « bottom » en sont une estimation au
    // nombre de lettres. Masqués à l'écran pendant un texte tapé (l'intro). Même règle dans la page
    // des moments (captionGroups).
    let (cap_max_words, cap_max_chars) = fonts_lib::caption_limits(&fonts_lib::read_style());
    let mut captions: Vec<Caption> = vec![];
    {
        let mut group: Vec<&WordEvent> = vec![];
        let flush = |group: &mut Vec<&WordEvent>, captions: &mut Vec<Caption>| {
            if group.is_empty() {
                return;
            }
            let words: Vec<String> = group.iter().map(|g| g.text.clone()).collect();
            let split = balanced_split(&words);
            captions.push(Caption {
                top: words[..split].join(" "),
                bottom: words[split..].join(" "),
                times: group.iter().map(|g| round_to(g.start, 3)).collect(), // début de chaque mot (mot prononcé en couleur)
                words,
                start: group[0].start,
                end: group[group.len() - 1].end,
            });
            group.clear();
        };
        for w in word_events.iter().filter(|w| !w.text.is_empty()) {
            let t = w.text.trim_end();
            let ends_sentence = t.ends_with('.') || t.ends_with('!') || t.ends_with('?');
            let ends_clause = t.ends_with(',');
            let gap = group.last().map(|g| w.start - g.end > 0.35).unwrap_or(false);
            if gap {
                flush(&mut group, &mut captions);
            }
            group.push(w);
            let chars: i64 = group.iter().map(|g| g.text.chars().count() as i64).sum();
            if ends_sentence || group.len() as i64 >= cap_max_words || chars > cap_max_chars || (ends_clause && group.len() >= 3) {
                flush(&mut group, &mut captions);
            }
        }
        flush(&mut group, &mut captions);
    }
    // Chaque sous-titre reste jusqu'au suivant (pas de trou à l'écran).
    // Sans déborder sur un plan sans parole qui suit.
    for k in 1..captions.len() {
        captions[k - 1].end = captions[k].start;
    }
    if let Some(last) = captions.last_mut() {
        // aucun sous-titre possible : moments sans sous-titres, plans sans parole
        last.end = total;
    }
    for cap in captions.iter_mut() {
        for (ci, c) in clips.iter().enumerate() {
            let (c_start, c_end) = (c.from as f64 / FPS, c.end());
            if c_start <= cap.start && cap.start < c_end {
                if ci + 1 < clips.len() && is_gap(clips[ci + 1].seg) {
                    cap.end = cap.end.min(c_end);
                }
                break;
            }
        }
    }

    // --- Zooms ----------------------------------------------------------------------
    // Alternance zoom sec (punch) / zoom in (progressif), plan changé toutes les ~1,8 s
    // sur une frontière de mot, pour que l'image ne soit jamais statique.
    let mut zooms: Vec<(f64, f64, &str)> = vec![];
    {
        let mut boundaries: Vec<f64> = word_events.iter().map(|w| round_to(w.start, 3)).chain(clips.iter().map(|c| c.from as f64 / FPS)).collect();
        boundaries.sort_by(|a, b| a.partial_cmp(b).unwrap());
        boundaries.dedup();
        let (mut t, mut kind) = (0.0f64, 0);
        while t < total - 0.3 {
            let target = t + 1.8;
            let mut nxt: Option<f64> = None;
            for &b in boundaries.iter().filter(|&&b| b >= t + 1.3) {
                if nxt.map(|n| (b - target).abs() < (n - target).abs()).unwrap_or(true) {
                    nxt = Some(b);
                }
            }
            let nxt = nxt.unwrap_or(total).min(total);
            zooms.push((round_to(t, 3), round_to(nxt, 3), if kind % 2 == 0 { "in" } else { "punch" }));
            kind += 1;
            t = nxt;
        }
        if let Some(z) = zooms.last_mut() {
            z.1 = total;
        }
    }

    // --- Temps de la page -> temps du montage -------------------------------------------
    // Sur la page, les temps d'un moment sont comptés depuis le début de son extrait
    // (0,05 s avant le segment de parole).
    // Un moment découpé a plusieurs clips.
    let mut clips_of: OMap<i64, Vec<usize>> = OMap::new();
    for (i, c) in clips.iter().enumerate() {
        clips_of.entry(c.seg, Vec::new).push(i);
    }
    keep.retain(|k| clips_of.contains(&k.seg)); // moment dont aucune partie n'est gardée

    // Clip du moment qui contient l'instant « at » de l'extrait (ou le plus proche).
    let clip_at = |seg_id: i64, at: f64| -> &Clip {
        let t = ctx.page_start(seg_id) + at;
        let mut best: Option<(f64, usize)> = None;
        for &i in clips_of.get(&seg_id).unwrap() {
            let c = &clips[i];
            let d = (c.trim_before as f64 / FPS - t).max(t - (c.trim_before + c.dur) as f64 / FPS).max(0.0);
            if best.map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, i));
            }
        }
        &clips[best.unwrap().1]
    };
    // Temps dans le montage d'un instant « at » de l'extrait du moment (borné au clip).
    let page_time = |seg_id: i64, at: f64| -> f64 {
        let c = clip_at(seg_id, at);
        let t = c.from as f64 / FPS + ctx.page_start(seg_id) + at - c.trim_before as f64 / FPS;
        t.max(c.from as f64 / FPS).min((c.from + c.dur - 1) as f64 / FPS)
    };
    let first_clip = |seg_id: i64| &clips[clips_of.get(&seg_id).unwrap()[0]];
    let last_clip = |seg_id: i64| &clips[*clips_of.get(&seg_id).unwrap().last().unwrap()];

    // --- Voix modifiée (qui tremble) -----------------------------------------------------
    // Par défaut sur les mots de FUNNY ; sur un moment dont les effets ont été choisis sur la
    // page, seulement si « voix » y est cochée (départ et durée en temps de la page).
    let mut funny: Vec<Funny> = vec![];
    let ids = voice_ids();
    let add_voice = |funny: &mut Vec<Funny>, seg_id: i64, at: f64, dur: f64, force: f64, voice: &str| {
        let s = page_time(seg_id, at);
        let e = (s + dur.max(0.05)).min(clip_at(seg_id, at + dur).end()).min(total);
        let force = force.max(0.0).min(2.0);
        if e > s && force > 0.0 {
            funny.push(Funny {
                start: round_to(s, 3),
                end: round_to(e, 3),
                force: round_to(force, 2),
                voice: if ids.contains(&voice) { voice.to_string() } else { "tremblement".into() },
            });
        }
    };
    let voice_of = |v: &Value| v.get("voice").and_then(Value::as_str).unwrap_or("tremblement").to_string();
    for k in &keep {
        let seg_id = k.seg;
        let default = voice_defaults.get(&seg_id);
        if !page_sfx.contains(&seg_id) || sfx_legacy.contains(&seg_id) {
            if let Some(d) = default {
                add_voice(&mut funny, seg_id, fnum(d, "at"), fnum(d, "dur"), 1.0, &voice_of(d));
            }
            continue;
        }
        for fx in page_sfx.get(&seg_id).unwrap() {
            if fx.as_str() == Some("voix") {
                let v = match default {
                    Some(d) => d.clone(),
                    None => {
                        let d: i64 = clips_of.get(&seg_id).unwrap().iter().map(|&i| clips[i].dur).sum();
                        json!({"at": 0.0, "dur": d as f64 / FPS})
                    }
                };
                add_voice(&mut funny, seg_id, fnum(&v, "at"), fnum(&v, "dur"), 1.0, &voice_of(&v));
            } else if fx.is_object() && fx.get("kind").and_then(Value::as_str) == Some("voix") {
                let force = match fx.get("force") {
                    None | Some(Value::Null) => 1.0,
                    f => pyfloat(f, 1.0),
                };
                let dur = if truthy(fx.get("dur")) { pyfloat(fx.get("dur"), 1.0) } else { 1.0 };
                let voice = if truthy(fx.get("voice")) { pystr(fx.get("voice")) } else { "tremblement".into() };
                add_voice(&mut funny, seg_id, pyfloat(fx.get("at"), 0.0), dur, force, &voice);
            }
        }
    }
    funny.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));

    // --- Bruitages ------------------------------------------------------------------
    let mut sfx: Vec<Sfx> = vec![];
    for &(seg_id, i) in &cash {
        if let Some(t) = word_time(seg_id, i) {
            // mot pas coupé
            sfx.push(Sfx { src: "sfx/cash.wav".into(), start: round_to(t, 3), volume: 0.55, dur: None });
        }
    }
    // Effets ajoutés sur la page. Sans réglage précis : au début du moment (la caisse par défaut
    // est déjà placée sur son mot ci-dessus). Avec réglage : « at » est compté depuis le début de
    // l'extrait du moment sur la page (0,05 s avant le segment), comme dans l'aperçu.
    // Sons par défaut (moments_lib::SOUNDS) : un déclenchement, sauf le clavier (6 frappes).
    let mut sfx_files: HashMap<String, (String, f64)> =
        SOUNDS.iter().filter_map(|(i, _, f, vol)| f.map(|f| (i.to_string(), (format!("sfx/{f}"), *vol)))).collect();
    let mut sfx_presets: HashMap<String, (i64, f64)> = SOUNDS.iter().map(|(i, _, _, _)| (i.to_string(), (1, 0.1))).collect();
    sfx_presets.insert("clavier".into(), (6, 0.08));
    // Sons personnels (public/sfx/perso/<nom>.wav) : un déclenchement par défaut, comme le bop.
    for n in util::list_dir(root.join("public").join("sfx").join("perso")) {
        if let Some(stem) = n.strip_suffix(".wav") {
            sfx_files.insert(format!("perso-{stem}"), (format!("sfx/perso/{n}"), 0.6));
            sfx_presets.insert(format!("perso-{stem}"), (1, 0.1));
        }
    }
    for (seg_id, i, kind) in &extra_sfx {
        if let (Some((src, vol)), Some(t)) = (sfx_files.get(kind), word_time(*seg_id, *i)) {
            sfx.push(Sfx { src: src.clone(), start: round_to(t, 3), volume: *vol, dur: None });
        }
    }
    for (seg_id, effects) in page_sfx.iter() {
        let seg_id = *seg_id;
        if !clips_of.contains(&seg_id) {
            continue;
        }
        // Sans réglage : au premier mot gardé (début du clip pour un plan sans parole).
        let t0 = word_events.iter().find(|w| w.seg == seg_id).map(|w| w.start).unwrap_or(first_clip(seg_id).from as f64 / FPS);
        for fx in effects {
            if fx.as_str() == Some("voix") || (fx.is_object() && fx.get("kind").and_then(Value::as_str) == Some("voix")) {
                continue; // voix modifiée : traitée sur la piste voix
            }
            let (kind, start, repeat, gap, dur, gain);
            if let Some(name) = fx.as_str() {
                if name == "cash" && cash.iter().any(|c| c.0 == seg_id) {
                    continue;
                }
                if name.starts_with("perso-") && !sfx_files.contains_key(name) {
                    continue; // son personnel retiré de la bibliothèque
                }
                let p = sfx_presets.get(name).copied().unwrap_or((1, 0.1));
                (kind, start, repeat, gap, dur, gain) = (name.to_string(), t0, p.0, p.1, None, 1.0);
            } else if let Some(p) = fx.get("kind").and_then(Value::as_str).and_then(|k| sfx_presets.get(k)) {
                kind = fx["kind"].as_str().unwrap().to_string();
                start = page_time(seg_id, pyfloat(fx.get("at"), 0.0));
                repeat = pyint(fx.get("repeat")).unwrap_or(p.0).clamp(1, 20);
                gap = pyfloat(fx.get("gap"), p.1).max(0.02);
                dur = if truthy(fx.get("dur")) { Some(pyfloat(fx.get("dur"), 0.0)) } else { None };
                gain = pyfloat(fx.get("vol"), 1.0).max(0.0).min(2.0); // volume réglé sur la page (1 = 100 %)
            } else {
                continue;
            }
            for k in 0..repeat {
                let (src, vol) = sfx_files.get(&kind).cloned().unwrap_or_else(|| (format!("sfx/key-{}.wav", k % 4 + 1), 0.45));
                sfx.push(Sfx {
                    src,
                    start: round_to(start + gap * k as f64, 3),
                    volume: round_to(vol * gain, 3),
                    dur: dur.map(|d| round_to(d, 3)),
                });
            }
        }
    }

    // --- Effets visuels : textes tapés et pastilles ------------------------------------
    let mut chip_defaults: HashMap<i64, (i64, String)> = HashMap::new();
    for (seg, i, label) in &chips_c {
        chip_defaults.insert(*seg, (*i, label.clone()));
    }
    // Effets visuels par défaut d'un moment, en temps de la page.
    let vfx_defaults = |seg_id: i64| -> Vec<Value> {
        let mut out = vec![];
        if !intro_text.is_empty() && keep.first().map(|k| k.seg) == Some(seg_id) {
            // intro tapée sur le 1er moment gardé
            let clip_len = if is_gap(seg_id) {
                keep[0].last.unwrap_or(0.0) // fin choisie du plan sans parole
            } else {
                ctx.seg.get(&seg_id).map(|s| fnum(s, "end")).unwrap_or(0.0) - ctx.page_start(seg_id) // jusqu'à la fin de la parole
            };
            out.push(json!({"kind": "typed", "text": intro_text, "at": intro_at, "type": intro_type,
                            "dur": num(round_to(clip_len - intro_at, 2)), "sound": true}));
        }
        if let Some(v) = extra_vfx.get(&seg_id) {
            out.extend(v.iter().cloned());
        }
        if let Some((i, label)) = chip_defaults.get(&seg_id) {
            if word_time(seg_id, *i).is_none() {
                return out; // mot de la pastille coupé
            }
            let at = fnum(&ctx.words(seg_id)[*i as usize], "a") - ctx.page_start(seg_id);
            out.push(json!({"kind": "chip", "text": label, "at": num(round_to(at, 3)), "dur": 2.2, "sound": true}));
        }
        out
    };

    let mut typed: Vec<Value> = vec![];
    let mut chips: Vec<Value> = vec![];
    // Effets visuels fournis par défaut (moments_lib::VISUALS) : zooms, secousses, flashs, mots mis
    // en valeur. « force » en % (zoom : grossissement ; secousse : amplitude).
    let mut zoom_fx: Vec<(f64, Value)> = vec![];
    let (mut shakes, mut flashes, mut highlights): (Vec<Value>, Vec<Value>, Vec<Value>) = (vec![], vec![], vec![]);
    for k in &keep {
        let seg_id = k.seg;
        let defaults = vfx_defaults(seg_id);
        let effects = page_vfx.get(&seg_id).unwrap_or(&defaults);
        for fx in effects {
            if !fx.is_object() {
                continue;
            }
            let kind = fx.get("kind").and_then(Value::as_str).unwrap_or("");
            if ["zoomin", "zoomsec", "dezoom", "shake", "flash"].contains(&kind) {
                let at = pyfloat(fx.get("at"), 0.0);
                let start = page_time(seg_id, at);
                let dur = pyfloat(fx.get("dur"), 0.8).max(0.05);
                let force = pyfloat(fx.get("force"), 25.0).min(150.0).max(0.0) / 100.0;
                match kind {
                    "zoomin" => {
                        // zoom progressif, gardé jusqu'à la fin de la partie du moment
                        let end = clip_at(seg_id, at).end();
                        zoom_fx.push((
                            round_to(start, 3),
                            json!({"type": "in", "start": r3(start), "anim": r3(dur), "end": r3(end), "force": num(force)}),
                        ));
                    }
                    "zoomsec" | "dezoom" => {
                        let t = if kind == "zoomsec" { "punch" } else { "out" };
                        zoom_fx.push((
                            round_to(start, 3),
                            json!({"type": t, "start": r3(start), "anim": r3(dur), "end": r3(start + dur), "force": num(force)}),
                        ));
                    }
                    "shake" => shakes.push(json!({"start": r3(start), "end": r3(start + dur), "force": num(force)})),
                    _ => flashes.push(json!({"start": r3(start), "end": r3(start + dur)})),
                }
                continue;
            }
            if kind == "zoomlibre" {
                // Zoom libre : d'un cadre à un autre (grossissement s, centre x, y de l'image visible),
                // entre deux instants du rush ; cadre de début avant, de fin après, sur tout le moment.
                let frame = |k: Option<&Value>| -> Value {
                    let empty = json!({});
                    let k = k.filter(|v| v.is_object()).unwrap_or(&empty);
                    let s = (if truthy(k.get("s")) { pyfloat(k.get("s"), 1.0) } else { 1.0 }).min(4.0).max(1.0);
                    let h = 0.5 / s;
                    json!({"s": r3(s), "x": num(round_to(pyfloat(k.get("x"), 0.5).min(1.0 - h).max(h), 4)),
                           "y": num(round_to(pyfloat(k.get("y"), 0.5).min(1.0 - h).max(h), 4))})
                };
                let at = pyfloat(fx.get("at"), 0.0);
                let a = ctx.page_start(seg_id) + at;
                let start = first_clip(seg_id).from as f64 / FPS;
                zoom_fx.push((
                    round_to(start, 3),
                    json!({"type": "key", "start": r3(start), "end": r3(last_clip(seg_id).end()), "a": r3(a),
                           "b": r3(a + pyfloat(fx.get("dur"), 1.0).max(0.05)),
                           "from": frame(fx.get("from")), "to": frame(fx.get("to"))}),
                ));
                continue;
            }
            if kind == "highlight" {
                // mot (ou mots) du sous-titre en couleur, sur tout le moment
                let word = if fx.get("text").is_some() { pystr(fx.get("text")) } else { String::new() };
                let word = word.trim();
                if !word.is_empty() {
                    let color = fx.get("color").and_then(Value::as_str).filter(|c| c.chars().count() == 7).unwrap_or("#ffd84d");
                    highlights.push(json!({"words": word, "color": color,
                                           "start": r3(first_clip(seg_id).from as f64 / FPS),
                                           "end": r3(last_clip(seg_id).end())}));
                }
                continue;
            }
            let text_s = if fx.get("text").is_some() { pystr(fx.get("text")) } else { String::new() };
            if text_s.trim().is_empty() {
                continue;
            }
            let text_v = fx.get("text").cloned().unwrap_or(Value::Null);
            let at = pyfloat(fx.get("at"), 0.0);
            let start = page_time(seg_id, at);
            let mut end = start + pyfloat(fx.get("dur"), 2.2).max(0.1); // peut déborder sur le moment suivant
            let sound = fx.get("sound").map(|v| truthy(Some(v))).unwrap_or(true);
            if kind == "typed" {
                // un texte tapé reste dans son moment (dans sa partie s'il est découpé)
                end = end.min(clip_at(seg_id, at).end());
                let type_dur = pyfloat(fx.get("type"), 1.6).max(0.1);
                typed.push(json!({"text": text_v, "start": r3(start), "typeDuration": num(type_dur), "end": r3(end)}));
                if sound {
                    // une frappe toutes les 2 lettres
                    let n_chars = (text_s.replace('\n', "").chars().count() as i64).max(1);
                    for k in (0..n_chars).step_by(2) {
                        sfx.push(Sfx {
                            src: format!("sfx/key-{}.wav", k / 2 % 4 + 1),
                            start: round_to(start + type_dur * k as f64 / n_chars as f64, 3),
                            volume: 0.45,
                            dur: None,
                        });
                    }
                }
            } else if kind == "chip" {
                chips.push(json!({"text": text_v, "start": r3(start), "end": r3(end)}));
                if sound {
                    sfx.push(Sfx { src: "sfx/bop.wav".into(), start: round_to(start, 3), volume: 0.7, dur: None });
                }
            }
        }
    }

    // Volume de tous les effets sonores (page des moments, work/effets_page.json « volume »,
    // 1 = 100 %), plafonné au volume maximal d'un son.
    let sfx_gain = match util::read_json(work.join("effets_page.json")) {
        Some(Value::Object(o)) => match o.get("volume") {
            None => 1.0,
            Some(v @ (Value::Number(_) | Value::Bool(_))) => pyfloat(Some(v), 1.0),
            Some(Value::String(s)) => s.trim().parse().unwrap_or(1.0),
            Some(_) => 1.0,
        },
        _ => 1.0,
    }
    .min(2.0)
    .max(0.0);
    for e in sfx.iter_mut() {
        e.volume = round_to((e.volume * sfx_gain).min(1.0), 3);
    }
    sfx.retain(|e| e.volume > 0.0);

    // --- Piste voix ------------------------------------------------------------------
    // Son de la page des moments (panneau « Son », work/son.json) : volume de la voix de tout le
    // montage, nettoyage (bruit de fond, clics, graves, clarté) et bruits parasites à atténuer
    // (fenêtres en temps du rush) ; volume de la voix par moment : « gain » de chaque moment.
    let mut sound = util::read_obj(work.join("son.json"));
    if derush {
        sound = Map::new();
    }
    let voice_gain = (if truthy(sound.get("voice")) { pyfloat(sound.get("voice"), 0.0) } else if sound.contains_key("voice") { 0.0 } else { 1.0 })
        .min(2.0)
        .max(0.0);
    let mute: Vec<(f64, f64)> = sound
        .get("mute")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let (x, y) = (pyfloat(p.get(0), 0.0), pyfloat(p.get(1), 0.0));
                    (y > x).then_some((x, y))
                })
                .collect()
        })
        .unwrap_or_default();
    util::mkdirs(root.join("public").join("audio"));
    let mut voice: Vec<f32> = vec![];
    for c in &clips {
        let s = (c.trim_before as f64 / FPS * SR as f64) as usize;
        let e = s + (c.dur as f64 / FPS * SR as f64) as usize;
        let (s0, e0) = (s.min(audio.len()), e.min(audio.len()));
        let g1 = voice_gain as f32;
        let g2 = page_gains.get(&c.seg).copied().unwrap_or(1.0) as f32;
        let mut part: Vec<f32> = audio[s0..e0].iter().map(|x| x * g1 * g2).collect();
        let n = part.len() as i64;
        for &(ma, mb) in &mute {
            // bruit parasite atténué de 15 dB, avec 10 ms de fondu de part et d'autre
            let (a, b) = ((ma * SR as f64) as i64 - s as i64, (mb * SR as f64) as i64 - s as i64);
            if b <= 0 || a >= n {
                continue;
            }
            let mut env = vec![1f32; part.len()];
            let ramp = (SR as f64 * 0.01) as i64;
            let (lo, hi) = ((a - ramp).max(0), (b + ramp).min(n));
            for x in lo..hi {
                env[x as usize] = 0.18f64 as f32;
            }
            for k in 0..ramp {
                if 0 <= a - ramp + k && a - ramp + k < n {
                    env[(a - ramp + k) as usize] = (1.0 - (1.0 - 0.18) * k as f64 / ramp as f64) as f32;
                }
                if 0 <= b + k && b + k < n {
                    env[(b + k) as usize] = (0.18 + (1.0 - 0.18) * k as f64 / ramp as f64) as f32;
                }
            }
            for (p, e) in part.iter_mut().zip(&env) {
                *p *= e;
            }
        }
        let fade = (SR as f64 * 0.004) as usize; // micro-fondu anti-clic sur les coupes sèches
        if part.len() >= fade {
            let up = linspace(0.0, 1.0, fade);
            let down = linspace(1.0, 0.0, fade);
            for k in 0..fade {
                part[k] = (part[k] as f64 * up[k]) as f32;
            }
            let off = part.len() - fade;
            for k in 0..fade {
                part[off + k] = (part[off + k] as f64 * down[k]) as f32;
            }
        }
        voice.extend(part);
    }
    let dry_path = work.join("voice_dry.wav");
    write_wav(&dry_path, &to_i16_f32(&voice))?;

    // Les graves parasites et la clarté sont faits au rendu par HyperFrames (selection/hf_audio.js,
    // comme dans l'aperçu de la page) ; le bruit de fond et les clics, ici par ffmpeg.
    let mut clean = String::new();
    let denoise = match pyint(sound.get("denoise")).unwrap_or(0) {
        1 => Some(6),
        2 => Some(12),
        3 => Some(20),
        _ => None,
    };
    if truthy(sound.get("declick")) {
        clean.push_str("adeclick=w=40:o=75,");
    }
    if let Some(d) = denoise {
        clean.push_str(&format!("afftdn=nr={d}:nf=-50:tn=1,"));
    }
    // 1. Nettoyage de toute la voix (panneau « Son de la voix »).
    let clean_path = work.join("voice_clean.wav");
    let filter = if clean.is_empty() { "anull".to_string() } else { clean.trim_end_matches(',').to_string() };
    let sr = SR.to_string();
    ffmpeg(&[
        "-v".into(), "error".into(), "-y".into(), "-i".into(), paths::display(&dry_path), "-af".into(), filter,
        "-ar".into(), sr.clone(), "-ac".into(), "1".into(), paths::display(&clean_path),
    ])?;

    // 2. Voix modifiées faites par ffmpeg (hors HF_VOICES, faites au rendu) : chaque fenêtre est
    //    traitée à part avec sa voix et sa force (moments_lib::voice_filter), puis remise en place
    //    avec de courts fondus enchaînés.
    let mut track = read_wav(&clean_path)?;
    let xf = (SR as f64 * 0.02) as usize;
    for f in &funny {
        if is_hf_voice(&f.voice) {
            continue;
        }
        let a = ((f.start * SR as f64) as i64 - xf as i64).max(0) as usize;
        let b = (((f.end * SR as f64) as i64 + xf as i64).max(0) as usize).min(track.len());
        if b < a || b - a < 2 * xf + 10 {
            continue;
        }
        let seg: Vec<f32> = track[a..b].to_vec();
        let (seg_in, seg_out) = (work.join("voice_fx_in.wav"), work.join("voice_fx_out.wav"));
        let mut graph = voice_filter(&f.voice, f.force);
        if f.voice == "batterie" {
            // ralentissement ici, puis retour à la durée d'origine
            let x: Vec<f64> = seg.iter().map(|&v| v as f64).collect();
            let slow = batterie_warp(&x, f.force);
            write_wav(&seg_in, &to_i16_f64(&slow))?;
            graph = graph.replacen("[0:a]", &format!("[0:a]rubberband=tempo={:.4},", slow.len() as f64 / seg.len() as f64), 1);
        } else {
            write_wav(&seg_in, &to_i16_f32(&seg))?;
        }
        ffmpeg(&[
            "-v".into(), "error".into(), "-y".into(), "-i".into(), paths::display(&seg_in), "-filter_complex".into(), graph,
            "-map".into(), "[r]".into(), "-ar".into(), sr.clone(), "-ac".into(), "1".into(), paths::display(&seg_out),
        ])?;
        let mut wet = read_wav(&seg_out)?;
        wet.resize(seg.len(), 0.0);
        let mut ramp = vec![1f32; seg.len()];
        for (k, v) in linspace(0.0, 1.0, xf).into_iter().enumerate() {
            ramp[k] = v as f32;
        }
        let off = seg.len() - xf;
        for (k, v) in linspace(1.0, 0.0, xf).into_iter().enumerate() {
            ramp[off + k] = v as f32;
        }
        for k in 0..seg.len() {
            track[a + k] = seg[k] * (1f32 - ramp[k]) + wet[k] * ramp[k];
        }
        let _ = fs::remove_file(&seg_in);
        let _ = fs::remove_file(&seg_out);
    }
    // Limiteur (-1,5 dB) : au rendu, par HyperFrames, après les voix modifiées.
    write_wav(&root.join("public").join("audio").join("voice.wav"), &to_i16_f32(&track))?;
    let _ = fs::remove_file(&clean_path);

    // --- Cadrage choisi par moment -------------------------------------------------------------
    // Moment cadré sur une personne (page des moments) : fenêtres en temps du montage, sur tous ses
    // plans ; le rendu y centre l'image sur cette personne (render_hyperframes, focusAt).
    let focus: Vec<Value> = clips
        .iter()
        .filter_map(|c| page_focus.get(&c.seg).map(|p| json!({"start": r3(c.from as f64 / FPS), "end": r3(c.end()), "person": p})))
        .collect();

    // --- Habillage de tout le montage ------------------------------------------------------
    let h = util::read_obj(work.join("habillage.json"));
    let mut habillage = json!({
        "autoZoom": num(pyfloat(h.get("autoZoom"), 1.0).min(2.0).max(0.0)),
        "pop": truthy(h.get("pop")),
        "karaoke": truthy(h.get("karaoke")),
        "karaokeColor": h.get("karaokeColor").and_then(Value::as_str).unwrap_or("#ffd84d"),
    });
    if derush {
        // image du rush telle quelle ; sous-titres simples si la case est cochée
        habillage["autoZoom"] = json!(0.0);
        habillage["pop"] = json!(false);
        habillage["karaoke"] = json!(false);
        zooms.clear();
        if !derush_subs {
            captions.clear();
        }
    }

    // --- Sorties -----------------------------------------------------------------------
    let caption_style = fonts_lib::copy_for_render(&fonts_lib::read_style()).unwrap_or(Value::Null);
    let (width, height) = frame_size(root);
    zoom_fx.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    sfx.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    let mut sfx_def = Map::new();
    for (k, v) in sfx_defaults.iter() {
        sfx_def.insert(k.to_string(), Value::Array(v.clone()));
    }
    let mut edit = Map::new();
    edit.insert("fps".into(), json!(fps_i));
    // Format de la composition, celui du rush (portrait 1080x1920 ou paysage 1920x1080).
    edit.insert("width".into(), json!(width));
    edit.insert("height".into(), json!(height));
    edit.insert("durationInFrames".into(), json!(total_frames));
    edit.insert("clips".into(), Value::Array(clips.iter().map(Clip::to_json).collect()));
    edit.insert("hdRanges".into(), json!(hd_ranges));
    // Effets proposés par défaut sur la page des moments (segment -> effets).
    edit.insert("sfxDefaults".into(), Value::Object(sfx_def));
    edit.insert("overlayDefaults".into(), Value::Object(overlay));
    edit.insert("typed".into(), Value::Array(typed));
    edit.insert(
        "captions".into(),
        Value::Array(
            captions
                .iter()
                .map(|c| {
                    json!({"top": c.top, "bottom": c.bottom, "words": c.words,
                           "times": c.times.iter().map(|&t| num(t)).collect::<Vec<_>>(),
                           "start": r3(c.start), "end": r3(c.end)})
                })
                .collect(),
        ),
    );
    // Style des sous-titres choisi sur la page des moments (fonts_lib).
    edit.insert("captionStyle".into(), caption_style);
    edit.insert(
        "zooms".into(),
        Value::Array(zooms.iter().map(|(s, e, t)| json!({"start": num(*s), "end": num(*e), "type": t})).collect()),
    );
    edit.insert("zoomFx".into(), Value::Array(zoom_fx.into_iter().map(|z| z.1).collect()));
    edit.insert("shakes".into(), Value::Array(shakes));
    edit.insert("flashes".into(), Value::Array(flashes));
    edit.insert("highlights".into(), Value::Array(highlights));
    // Habillage de tout le montage (page des moments, work/habillage.json) : intensité des
    // zooms automatiques, sous-titres qui rebondissent, mot prononcé en couleur.
    edit.insert("habillage".into(), habillage);
    edit.insert(
        "funny".into(),
        Value::Array(
            funny
                .iter()
                .map(|f| json!({"start": num(f.start), "end": num(f.end), "force": num(f.force), "voice": f.voice}))
                .collect(),
        ),
    );
    edit.insert("focus".into(), Value::Array(focus));
    // Nettoyage fait au rendu par HyperFrames (selection/hf_audio.js) : graves parasites, clarté.
    edit.insert("son".into(), json!({"highpass": truthy(sound.get("highpass")), "clarity": truthy(sound.get("clarity"))}));
    edit.insert("chips".into(), Value::Array(chips));
    edit.insert(
        "sfx".into(),
        Value::Array(
            sfx.iter()
                .map(|e| {
                    let mut m = Map::new();
                    m.insert("src".into(), json!(e.src));
                    m.insert("start".into(), num(e.start));
                    m.insert("volume".into(), num(e.volume));
                    if let Some(d) = e.dur {
                        m.insert("dur".into(), num(d));
                    }
                    Value::Object(m)
                })
                .collect(),
        ),
    );
    // Sélection et réglages de la page montés ici (use_selection) : la page des moments
    // signale les moments changés depuis cette version de travail.
    edit.insert("page".into(), selection.as_ref().and_then(|s| s.get("page").cloned()).unwrap_or(Value::Null));
    util::mkdirs(paths::data());
    let text = pyjson::dumps_with(&Value::Object(edit), pyjson::Opts { indent: Some(1), ensure_ascii: false, ..Default::default() });
    fs::write(paths::data().join("edit.json"), text).map_err(|e| format!("{} : {e}", paths::data().join("edit.json").display()))?;

    println!("{} clips, {} images = {:.1} s", clips.len(), total_frames, total);
    println!("{} sous-titres, {} zooms, {} moments gênés, {} bruitages", captions.len(), zooms.len(), funny.len(), sfx.len());
    for c in &captions {
        println!("  {:6.2} {} / {}", c.start, c.top, c.bottom);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linspace_like_numpy() {
        let v = linspace(0.0, 1.0, 5);
        assert_eq!(v, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
        let d = linspace(1.0, 0.0, 3);
        assert_eq!(d, vec![1.0, 0.5, 0.0]);
    }

    #[test]
    fn pairwise_sum_small_and_large() {
        let a: Vec<f32> = (0..480).map(|i| i as f32).collect();
        assert_eq!(pairwise_sum(&a), (0..480).sum::<i32>() as f32);
        assert_eq!(pairwise_sum(&[1.0, 2.0, 3.0]), 6.0);
    }

    #[test]
    fn balanced_split_by_letters() {
        let w: Vec<String> = ["Bonjour", "à", "tous"].iter().map(|s| s.to_string()).collect();
        assert_eq!(balanced_split(&w), 1);
        assert_eq!(balanced_split(&w[..1]), 0);
    }

    #[test]
    fn realign_keeps_timing_slots() {
        let words: Vec<Value> = ["bonjour", "a", "tous"].iter().map(|w| json!({"w": w})).collect();
        assert_eq!(realign(&words, "Bonjour à tous"), vec!["Bonjour", "à", "tous"]);
        assert_eq!(realign(&words, "Bonjour tous mes amis"), vec!["Bonjour", "", "tous mes amis"]);
    }
}
