//! Rendu du montage avec HyperFrames : le montage (src/data/edit.json, face.json) est écrit en
//! page HTML + GSAP dans work/rendu_hyperframes/ puis rendu par « hyperframes render »
//! (Chrome sans écran, ffmpeg). Effets dessinés image par image en JavaScript (zooms, secousses,
//! flashs, textes tapés, pastilles, sous-titres avec mots mis en valeur, mot prononcé en couleur et
//! rebond) ; la coupure des sous-titres en 2 lignes est calculée ici (render_lib).
//!
//! Usage : auto-montage render_hyperframes apercu|final <sortie.mp4>
//!   final : source 4K (make_hd), page rendue à l'échelle 2 (2160x3840, 3840x2160 en paysage,
//!   2880x2160 en 4:3…).

use crate::memoire::{self, VideoInfo};
use crate::moments_lib::{frame_layout, is_hf_voice};
use crate::paths;
use crate::place;
use crate::procs;
use crate::progress::{report, report_every, say};
use crate::pyjson;
use crate::render_lib::{poppins, Montage};
use crate::util::{self, round_int, round_to};
use regex::Regex;
use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

fn dir() -> PathBuf {
    paths::work().join("rendu_hyperframes")
}
fn hf_audio() -> PathBuf {
    paths::root().join("selection").join("hf_audio.js")
}
fn hf_look() -> PathBuf {
    paths::root().join("selection").join("hf_look.js")
}
fn parts_dir() -> PathBuf {
    paths::work().join("rendu_parties") // parties d'un long montage déjà rendues
}
const PART_SECONDS: f64 = memoire::RENDER_PART; // durée d'une partie (secondes de montage)
// Disque pris par le rendu (partie la plus longue raccourcie pour qu'il tienne, disk_part) :
// - images des plans d'une partie, extraites par HyperFrames (JPEG qualité 95) avant de la rendre :
//   0,06 à 0,16 octet par pixel mesurés (rush de synthèse, rush filmé), 0,2 par prudence ;
// - mémoire partagée de chaque navigateur, écrite dans /tmp (Chrome, quand /dev/shm est petit, comme
//   dans un conteneur) : 2 Go mesurés pour 4 navigateurs en 1080p, environ 250 octets par pixel ;
// - vidéos rendues : les parties (pendant le rendu), puis aussi le montage assemblé (à la fin, sans
//   images extraites ni navigateurs), environ 1 Mo/s en 1080p chacun.
const FRAME_BYTES: f64 = 0.2;
const BROWSER_BYTES: f64 = 300.0;
const OUT_BYTES: f64 = 0.5; // par pixel et par seconde de montage
const DISK_SHARE: f64 = 0.8; // part du disque libre que le rendu peut prendre
const MIN_PART: f64 = 20.0; // secondes de montage : partie la plus courte, sinon disque trop plein pour rendre
// 4K : chaque navigateur garde en mémoire les images des plans qu'il a affichées, environ 4 Mo par
// image 4K (0,5 octet par pixel de l'image rendue, mesuré en 16:9 ; 30 fois moins par pixel en
// 1080p), jusqu'à la fin de la partie. Une partie de 1 min 30 dépassait 7,7 Go et le rendu
// s'arrêtait (« capture stalled », code 1) : la partie est raccourcie pour que cette mémoire tienne
// à côté du reste du rendu (memory_part), sans descendre sous MIN_MEM_PART.
const CAPTURE_BYTES: f64 = 0.5;
const MIN_MEM_PART: f64 = 30.0;
// Surveillance pendant le rendu : mémoire libre sous LOW_MB ou LOW_SHARE de la mémoire totale
// pendant LOW_SECONDS de suite (l'estimation n'a pas suffi, ou un autre programme en a pris) :
// node et ses navigateurs sont arrêtés et la partie est refaite avec moins de navigateurs, puis en
// deux parties ; sinon la machine s'enlisait ou le système tuait node et laissait ses navigateurs.
const LOW_MB: f64 = 1000.0;
const LOW_SHARE: f64 = 0.08;
const LOW_SECONDS: u32 = 2;
const LOW_MEMORY: i32 = i32::MIN; // code de run_hyperframes : arrêté par la surveillance

fn cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}
// Extractions d'images des plans en même temps (ffmpeg_limite) : environ 200 Mo chacune en 1080p
// (4 fois plus en 4K : 2 au plus).
fn extractions(hd: bool) -> usize {
    cpus().clamp(1, if hd { 2 } else { 4 })
}
fn node_modules() -> PathBuf {
    paths::root().join("node_modules")
}
/// 4K : préréglages de « hyperframes render --resolution » (même page, rendue à l'échelle 2) ; les
/// autres formats (4:3…) sont agrandis dans la page elle-même (build, k = 2).
fn hd_preset(w: i64, h: i64) -> Option<&'static str> {
    match (w, h) {
        (1080, 1920) => Some("portrait-4k"),
        (1920, 1080) => Some("landscape-4k"),
        (1080, 1080) => Some("square-4k"),
        _ => None,
    }
}

/// Durée d'un WAV en secondes (1e9 s'il est illisible).
fn wav_len(path: &Path) -> f64 {
    match hound::WavReader::open(path) {
        Ok(r) => r.duration() as f64 / r.spec().sample_rate as f64,
        Err(_) => 1e9,
    }
}

/// `str(x)` de Python pour une valeur JSON (nombre entier ou flottant, texte…).
fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::String(s) => s.clone(),
        Value::Number(_) => pyjson::dumps(v),
        _ => pyjson::dumps(v),
    }
}

/// `f"{x:.6f}".rstrip("0").rstrip(".")`
fn r6(x: f64) -> String {
    let s = format!("{x:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn num(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(0.0)
}

/// Plans du montage : [(première image, image de fin)] ; transition vers le plan i+1 (look.rs) :
/// le plan i continue dessous pendant « frames » images.
fn clip_spans(edit: &Value) -> Vec<(f64, f64)> {
    let mut longer: HashMap<usize, f64> = HashMap::new();
    for t in util::arr(edit, "transitions") {
        let i = t.get("clip").and_then(Value::as_u64).unwrap_or(0) as usize;
        if i > 0 {
            longer.insert(i - 1, num(t.get("frames")));
        }
    }
    util::arr(edit, "clips")
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let from = num(c.get("from"));
            (from, from + num(c.get("durationInFrames")) + longer.get(&i).copied().unwrap_or(0.0))
        })
        .collect()
}

/// Plans qui passent dans les images [f0, f1) : ceux de la page de cette partie (build).
fn clips_in(spans: &[(f64, f64)], f0: i64, f1: i64) -> usize {
    spans.iter().filter(|&&(a, b)| a.max(f0 as f64) < b.min(f1 as f64)).count()
}

/// Fichier du projet HyperFrames (lien vers l'original, sans copie quand c'est possible).
fn link(src: &Path, name: &str) -> String {
    let mut dst = dir().join("assets");
    for part in name.split('/') {
        dst = dst.join(part);
    }
    if let Some(parent) = dst.parent() {
        util::mkdirs(parent);
    }
    if fs::symlink_metadata(&dst).is_ok() {
        let _ = fs::remove_file(&dst);
    }
    if fs::hard_link(src, &dst).is_err() {
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(src, &dst).is_ok();
        #[cfg(not(unix))]
        let linked = false;
        if !linked {
            let _ = fs::copy(src, &dst);
        }
    }
    format!("assets/{name}")
}

/// Éléments audio de la voix, avec les voix modifiées de HF_VOICES, les graves, la clarté et
/// le limiteur faits par HyperFrames : selection/hf_audio.js (le même code que l'aperçu de la
/// page des moments), lancé avec node. Pistes à partir de 1 ; les bruitages sont sur les pistes 20 et plus.
fn voice_audio(src: &str, duration: f64, e: &Value, t0: f64) -> Result<String, String> {
    let son = match e.get("son") {
        Some(v) if util::truthy(Some(v)) => v.clone(),
        _ => json!({}),
    };
    let windows: Vec<Value> = util::arr(e, "funny")
        .iter()
        .filter(|f| f.get("voice").and_then(Value::as_str).map(is_hf_voice).unwrap_or(false))
        .map(|f| {
            json!({"a": num(f.get("start")) - t0, "b": num(f.get("end")) - t0, "voice": f["voice"].clone(),
                   "force": f.get("force").cloned().unwrap_or(json!(1))})
        })
        .collect();
    let spec = json!({"src": src, "start": 0, "mediaStart": t0, "duration": duration, "volume": 1, "track": 1,
                      "son": son, "windows": windows});
    let js = "const H = require(process.argv[1]); let s = ''; process.stdin.on('data', (d) => s += d);\
              process.stdin.on('end', () => process.stdout.write(H.voiceElements(JSON.parse(s))));";
    let mut child = Command::new(paths::node())
        .args(["-e", js])
        .arg(hf_audio())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("node ne se lance pas : {e}"))?;
    let input = pyjson::dumps(&spec);
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    if !out.status.success() {
        return Err(format!(
            "selection/hf_audio.js (node) a échoué ({}) : {}",
            procs::py_code(&out.status),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Page du montage ; k : agrandissement fait dans la page (2 pour une 4K sans préréglage
/// « --resolution » de HyperFrames : 4:3 et autres formats que 9:16, 16:9 et carré).
/// part : (première image, image de fin) d'une partie du montage, rendue seule (main).
fn build(m: &mut Montage, k: i64, part: Option<(i64, i64)>) -> Result<(), String> {
    let fps_v = m.edit.get("fps").cloned().unwrap_or(json!(30));
    let fps = m.fps;
    let n = m.frames;
    let (f0, f1) = part.unwrap_or((0, n));
    let (t0, length) = (f0 as f64 / fps, (f1 - f0) as f64 / fps);
    let (rush, key) = m.rush();
    let rush_name = rush.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let src = link(&rush, &rush_name);
    // Plans : un <video> par plan (chaque élément a un id : sans lui, HyperFrames le fige ou le
    // coupe), début arrondi à l'image inférieure, lecture un quart d'image après le début du plan
    // (l'image exacte du plan, sans glisser sur la précédente).
    // Partie : les plans qui y passent, coupés à ses bords (clip_spans : prolongés pendant leur
    // transition). Chaque plan dans son cadre (box : styles de la transition) et son zoom (zb :
    // cadrage), sur deux pistes en alternance (deux plans à la fois pendant une transition).
    let spans = clip_spans(&m.edit);
    let mut videos = vec![];
    for (i, c) in util::arr(&m.edit, "clips").iter().enumerate() {
        let (from, end) = spans[i];
        let (a, b) = (from.max(f0 as f64), end.min(f1 as f64));
        if b > a {
            let start = ((a - f0 as f64) / fps * 1e6).trunc() / 1e6;
            videos.push(format!(
                "<div class=\"fill box\" id=\"box{i}\"><div class=\"fill zoom\" id=\"zb{i}\">\
                 <video id=\"clip{i}\" class=\"clip\" muted playsinline src=\"{src}\" data-start=\"{}\" \
                 data-duration=\"{}\" data-media-start=\"{}\" data-track-index=\"{}\"></video></div></div>",
                pyjson::float_repr(start),
                r6((b - a) / fps),
                r6((num(c.get(key)) + a - from + 0.25) / fps),
                i % 2
            ));
        }
    }
    let videos = videos.join("\n");
    let voice = paths::public().join("audio").join("voice.wav");
    let voice_src = link(&voice, "voice.wav");
    let mut audios = vec![voice_audio(&voice_src, length.min(wav_len(&voice) - t0), &m.edit, t0)?];
    let public = paths::public();
    for (i, s) in util::arr(&m.edit, "sfx").iter().enumerate() {
        // Même calcul que Montage.sfx (volume gardé tel qu'écrit : entier ou flottant).
        let start = round_int(num(s.get("start")) * fps) as f64 / fps;
        let dur = round_int(fps * s.get("dur").and_then(Value::as_f64).unwrap_or(1.5)).max(1) as f64 / fps;
        let vol = s.get("volume").filter(|v| v.is_number()).cloned().unwrap_or(json!(1)); // nombre seulement (attribut)
        let rel = s.get("src").and_then(Value::as_str).unwrap_or("");
        let path = rel.split('/').fold(public.clone(), |p, x| p.join(x));
        // Bruitage commencé avant la partie : sa suite (data-media-start).
        let start = start - t0;
        let skip = (-start).max(0.0);
        let dur = (dur - skip).min(wav_len(&path) - skip).min(length - start.max(0.0));
        if dur > 0.0 {
            let rel_norm: Vec<&str> = rel.split('/').filter(|x| !x.is_empty() && *x != ".").collect();
            let name = format!("sfx/{}", rel_norm.join("_"));
            audios.push(format!(
                "<audio id=\"sfx{i}\" src=\"{}\" data-start=\"{}\" data-duration=\"{}\" \
                 data-media-start=\"{}\" data-track-index=\"{}\" data-volume=\"{}\"></audio>",
                link(&path, &name),
                r6(start.max(0.0)),
                r6(dur),
                r6(skip),
                20 + i % 8,
                py_str(&vol)
            ));
        }
    }
    // Sous-titres : mots, couleurs des mots mis en valeur, lignes (et leur taille) et réduction.
    let mut caps = vec![];
    let captions: Vec<Value> = util::arr(&m.edit, "captions").to_vec();
    for c in &captions {
        let words = m.caption_words(c);
        let colors = m.caption_colors(c, &words);
        let big: Vec<bool> = colors.iter().map(Option::is_some).collect();
        let (lines, scale) = m.caption_layout(&words, &big);
        let scale = util::num(scale);
        let times = match c.get("times") {
            Some(t) if util::truthy(Some(t)) => t.clone(),
            _ => json!([]),
        };
        caps.push(json!({
            "start": c.get("start").cloned().unwrap_or(Value::Null),
            "end": c.get("end").cloned().unwrap_or(Value::Null),
            "words": words,
            "colors": colors,
            "lines": lines.iter().map(|&(a, b, s)| json!([a, b, util::num(s)])).collect::<Vec<_>>(),
            "scale": scale,
            "times": times,
        }));
    }
    let font_ext = m.font_file.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let e = &m.edit;
    let or_empty = |k: &str| match e.get(k) {
        Some(v) if util::truthy(Some(v)) => v.clone(),
        _ => json!([]),
    };
    let focus = util::truthy(e.get("focus"));
    let face: Vec<Value> = m
        .face
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
        .iter()
        .map(|f| {
            let mut o = Map::new();
            for k in ["t", "x", "y", "s"] {
                o.insert(k.into(), f.get(k).cloned().unwrap_or(json!(3)));
            }
            if focus {
                o.insert("faces".into(), f.get("faces").cloned().unwrap_or(json!([])));
            }
            Value::Object(o)
        })
        .collect();
    let clips: Vec<Value> = util::arr(e, "clips")
        .iter()
        .map(|c| {
            let mut o = Map::new();
            for k in ["from", "durationInFrames", "trimBefore"] {
                o.insert(k.into(), c.get(k).cloned().unwrap_or(Value::Null));
            }
            Value::Object(o)
        })
        .collect();
    let mut cap = Map::new();
    for k in ["weight", "size", "uppercase", "color", "shadow", "blur", "ox", "oy", "posX", "posY"] {
        cap.insert(k.into(), m.cap.get(k).cloned().unwrap_or(Value::Null));
    }
    let data = json!({
        "fps": fps_v, "frames": n, "clips": clips, "face": face,
        "focus": or_empty("focus"), "zooms": or_empty("zooms"), "zoomFx": or_empty("zoomFx"), "shakes": or_empty("shakes"),
        "flashes": or_empty("flashes"), "typed": Value::Array(m.typed.clone()), "chips": or_empty("chips"), "captions": caps,
        "couleur": e.get("couleur").cloned().unwrap_or(Value::Null), "looks": or_empty("looks"), "transitions": or_empty("transitions"),
        "hab": Value::Object(m.hab.clone()), "cap": Value::Object(cap),
    });
    // Format du rush : sur une image basse (paysage, 4:3), sous-titres plus bas et texte tapé au centre.
    let pos = frame_layout(m.width, m.height);
    let poppins_src = link(&poppins(), "fonts/poppins.ttf");
    let capfont = link(&m.font_file.clone(), &format!("fonts/caption{font_ext}"));
    let reps: Vec<(&str, String)> = vec![
        ("W", m.width.to_string()),
        ("H", m.height.to_string()),
        ("OUTW", (k * m.width).to_string()),
        ("OUTH", (k * m.height).to_string()),
        ("K", k.to_string()),
        ("CAPTOP", py_str(&pos["capTop"])),
        ("TYPEDTOP", py_str(&pos["typedTop"])),
        ("FPS", py_str(&fps_v)),
        ("DUR", r6(length)),
        ("OFF", f0.to_string()),
        ("VIDEOS", videos),
        ("AUDIOS", audios.join("\n")),
        ("POPPINS", poppins_src),
        ("CAPFONT", capfont),
        ("CAPWEIGHT", m.font_weight.to_string()),
        ("DATA", pyjson::dumps_utf8(&data).replace('<', "\\u003c")),
    ];
    let mut page = PAGE.to_string();
    for (k, v) in reps {
        page = page.replace(&format!("%{k}%"), &v);
    }
    fs::write(dir().join("index.html"), page).map_err(|e| format!("work/rendu_hyperframes/index.html : {e}"))?;
    fs::copy(node_modules().join("gsap").join("dist").join("gsap.min.js"), dir().join("assets").join("gsap.min.js"))
        .map_err(|e| format!("gsap.min.js (paquet npm gsap) : {e}"))?;
    // Couleurs, effets d'image et transitions : le même code que l'aperçu de la page des moments.
    fs::copy(hf_look(), dir().join("assets").join("hf_look.js")).map_err(|e| format!("selection/hf_look.js : {e}"))?;
    Ok(())
}

/// Programme « ffmpeg_limite » que HyperFrames appelle : ce programme-ci, copié (ou lié) sous ce
/// nom dans work/rendu_hyperframes/bin/ (main.rs le reconnaît à son nom).
fn limiter_exe() -> Result<PathBuf, String> {
    let bin = dir().join("bin");
    util::mkdirs(&bin);
    let dst = bin.join(if cfg!(windows) { "ffmpeg_limite.exe" } else { "ffmpeg_limite" });
    let exe = paths::self_exe();
    let exe = paths::canon(&exe).unwrap_or(exe);
    util::link_or_copy(&exe, &dst).map_err(|e| format!("ffmpeg_limite ({}) : {e}", dst.display()))?;
    Ok(dst)
}

/// ffmpeg, ffprobe (@ffprobe-installer) et Chrome sans écran (render_lib) : variables
/// d'environnement de « hyperframes render ».
fn tools(hd: bool) -> Result<Vec<(String, String)>, String> {
    let ff = paths::display(paths::ffmpeg());
    let mut env: Vec<(String, String)> = vec![
        ("HYPERFRAMES_NO_TELEMETRY".into(), "1".into()),
        ("DO_NOT_TRACK".into(), "1".into()),
        ("HYPERFRAMES_NO_UPDATE_CHECK".into(), "1".into()),
        ("HYPERFRAMES_NO_AUTO_INSTALL".into(), "1".into()),
    ];
    // Extraction des images des plans : un ffmpeg par plan, tous en même temps chez HyperFrames
    // (plus de 10 Go pour 150 plans) ; ffmpeg_limite en laisse passer extractions() à la fois.
    // L'attente compte dans le délai de chaque ffmpeg (5 min par défaut) : porté à 6 h.
    // Page d'un long montage (150 plans : 35 à 45 s par navigateur) : plus que les 45 s permises
    // par défaut pour être prête.
    env.push(("PRODUCER_PLAYER_READY_TIMEOUT_MS".into(), (15 * 60 * 1000).to_string()));
    // Marque héritée par les navigateurs, pour les arrêter même hors du groupe de la tâche.
    env.push((procs::RENDER_MARK.into(), "1".into()));
    // Images extraites des plans : par défaut dans un cache de /tmp que HyperFrames ne vide qu'au
    // bout d'une heure, toutes parties comprises (un montage de 22 min : plus de 13 Go, disque
    // plein). Sans cache, celles d'une partie vont dans son dossier de travail, effacé à sa fin.
    env.push(("HYPERFRAMES_EXTRACT_CACHE_DIR".into(), "off".into()));
    // Images capturées envoyées à l'encodeur au fil du rendu, même à plusieurs navigateurs en
    // capture par copie d'écran (4K par « --resolution » : image agrandie dans le navigateur, sans
    // BeginFrame). Sinon HyperFrames écrit chaque image sur le disque avant d'encoder : environ
    // 4 Mo par image 4K, 18 Go pour une partie de 2 min 30, et le rendu s'arrête (code 1).
    env.push(("HF_CAPTURE_PARALLEL_STREAM".into(), "true".into()));
    // ffmpeg_limite (verrous de fichiers, sur les trois systèmes) : son chemin est donné à chaque
    // partie (limiter_exe, le dossier du rendu est refait) ; ici le vrai ffmpeg par défaut.
    env.push(("AM_FFMPEG_REAL".into(), ff.clone()));
    env.push(("AM_FFMPEG_SLOTS".into(), extractions(hd).to_string()));
    env.push(("AM_FFMPEG_LOCKS".into(), paths::display(&dir().join("verrous"))));
    env.push(("FFMPEG_PROCESS_TIMEOUT_MS".into(), (6u64 * 3600 * 1000).to_string()));
    env.push(("HYPERFRAMES_FFMPEG_PATH".into(), ff));
    let probe = paths::ffprobe();
    if !probe.is_file() {
        return Err("ffprobe introuvable (paquet npm @ffprobe-installer/ffprobe) : relancer « npm install ».".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&probe, fs::Permissions::from_mode(0o755));
    }
    env.push(("HYPERFRAMES_FFPROBE_PATH".into(), paths::display(&probe)));
    if let Some(shell) = crate::render_lib::headless_chrome() {
        env.push(("PRODUCER_HEADLESS_SHELL_PATH".into(), paths::display(&shell)));
    }
    Ok(env)
}

static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").unwrap());
static PCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,3})%\s+(\S.*)$").unwrap());

/// Une ligne de « hyperframes render » : avancement (pourcentage) ou message recopié.
fn hf_line(raw: &[u8], done: f64, part_len: f64, total: f64) {
    let text = String::from_utf8_lossy(raw);
    let text = ANSI.replace_all(&text, "");
    let text = text.trim();
    if let Some(c) = PCT.captures(text) {
        let pct = c[1].parse::<f64>().unwrap_or(0.0).min(100.0);
        report("Rendu de la vidéo", round_to(done + pct * part_len / 100.0, 2), total);
    } else if !text.is_empty()
        && !text.starts_with("[INFO]")
        && !text.starts_with('|')
        && !text.starts_with("o ")
        && !text.chars().all(|c| matches!(c, '█' | '░' | ' '))
    {
        say(text);
    }
}

/// Lit une sortie du programme morceau par morceau, coupée aux \r et \n.
fn pump(mut r: impl Read, done: f64, part_len: f64, total: f64) {
    let mut buf: Vec<u8> = vec![];
    let mut chunk = [0u8; 4096];
    loop {
        let n = match r.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        while let Some(i) = buf.iter().position(|&b| b == b'\r' || b == b'\n') {
            let line: Vec<u8> = buf.drain(..=i).collect();
            hf_line(&line[..line.len() - 1], done, part_len, total);
        }
    }
    hf_line(&buf, done, part_len, total);
}

/// Mémoire libre presque épuisée (LOW_MB, LOW_SHARE).
fn low_memory() -> bool {
    let (Some(avail), total) = memoire::available() else { return false };
    (avail as f64) < (LOW_MB * memoire::MO).max(LOW_SHARE * total.unwrap_or(0) as f64)
}

/// Rend une page ; avancement en secondes de montage (done : déjà rendu avant cette partie).
/// watch : arrêt de node et de ses navigateurs si la mémoire libre s'épuise (code LOW_MEMORY).
/// Navigateurs restés après la fin de node arrêtés dans tous les cas.
fn run_hyperframes(cmd: &[String], env: &[(String, String)], done: f64, part_len: f64, total: f64, watch: bool) -> i32 {
    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..]).current_dir(paths::root()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => {
            eprintln!("hyperframes ne se lance pas ({}) : {e}", cmd[0]);
            return 127;
        }
    };
    let finished = Arc::new(AtomicBool::new(false));
    let tripped = Arc::new(AtomicBool::new(false));
    let watcher = watch.then(|| {
        let (finished, tripped) = (finished.clone(), tripped.clone());
        std::thread::spawn(move || {
            let mut low = 0;
            while !finished.load(Ordering::SeqCst) {
                low = if low_memory() { low + 1 } else { 0 };
                if low >= LOW_SECONDS {
                    tripped.store(true, Ordering::SeqCst);
                    procs::kill_marked(procs::RENDER_MARK);
                    return;
                }
                for _ in 0..10 {
                    if finished.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        })
    });
    let err = child.stderr.take().unwrap();
    let t = std::thread::spawn(move || pump(err, done, part_len, total));
    pump(child.stdout.take().unwrap(), done, part_len, total);
    let _ = t.join();
    let code = procs::wait(&mut child);
    finished.store(true, Ordering::SeqCst);
    if let Some(w) = watcher {
        let _ = w.join();
    }
    procs::kill_marked(procs::RENDER_MARK);
    if tripped.load(Ordering::SeqCst) {
        LOW_MEMORY
    } else {
        code
    }
}

/// Durée de partie la plus longue (secondes de montage) dont les images extraites tiennent dans
/// DISK_SHARE du disque libre, à côté des navigateurs et des parties déjà rendues (pixels : ceux
/// de l'image rendue et de la source) ; None si le disque ne limite pas (1,5 fois PART_SECONDS).
fn disk_part(fps: f64, pixels: f64, free: f64, workers: usize, total: f64) -> Option<f64> {
    let room = DISK_SHARE * free - workers as f64 * BROWSER_BYTES * pixels - total * OUT_BYTES * pixels;
    let longest = room / (fps * pixels * FRAME_BYTES);
    if longest >= 1.5 * PART_SECONDS {
        None
    } else {
        Some(longest)
    }
}

/// Durée de partie la plus longue (secondes de montage) dont les images gardées par les
/// navigateurs tiennent dans la mémoire libre, à côté du reste du rendu ; None si la mémoire ne
/// limite pas (1080p, ou mémoire libre inconnue).
fn memory_part(fps: f64, pixels: f64, hd: bool, info: &VideoInfo, workers: usize) -> Option<f64> {
    let avail = memoire::available().0?;
    if !hd {
        return None;
    }
    let v = VideoInfo { montage: 0.0, workers: Some(workers), ..info.clone() };
    let base = memoire::need("Rendu 4K", &v, &memoire::load_measures()) as f64;
    let longest = MIN_MEM_PART.max((memoire::RENDER_SHARE * avail as f64 - base) / (fps * pixels * CAPTURE_BYTES));
    if longest >= 1.5 * PART_SECONDS {
        None
    } else {
        Some(longest)
    }
}

/// Parties du rendu : [(première image, image de fin)] d'environ PART_SECONDS chacune ; une
/// seule pour un montage de moins de 1,5 fois cette durée. longest : durée la plus longue
/// permise par le disque (disk_part).
fn parts_of(frames: i64, fps: f64, longest: Option<f64>) -> Vec<(i64, i64)> {
    let f = frames as f64;
    let mut count = if f > 1.5 * fps * PART_SECONDS { round_int(f / (fps * PART_SECONDS)).max(1) } else { 1 };
    if let Some(l) = longest.filter(|l| *l != 0.0) {
        count = count.max((f / (fps * l)).ceil() as i64);
    }
    let bounds: Vec<i64> = (0..=count).map(|i| round_int(i as f64 * f / count as f64)).collect();
    bounds.windows(2).map(|w| (w[0], w[1])).collect()
}

/// Parties recoupées au début d'un plan pour que la page de chacune en ait au plus `most`
/// (mémoire des navigateurs : memoire::render_plan) ; une partie sans début de plan où couper
/// reste entière.
fn split_by_clips(parts: Vec<(i64, i64)>, spans: &[(f64, f64)], most: usize) -> Vec<(i64, i64)> {
    let most = most.max(1);
    let mut out = vec![];
    for (mut a, b) in parts {
        while clips_in(spans, a, b) > most {
            let mut starts: Vec<i64> = spans.iter().map(|s| s.0.ceil() as i64).filter(|&f| f > a && f < b).collect();
            starts.sort_unstable();
            starts.dedup();
            let Some(cut) = starts.iter().copied().filter(|&c| clips_in(spans, a, c) <= most).max().or(starts.first().copied()) else {
                break;
            };
            out.push((a, cut));
            a = cut;
        }
        out.push((a, b));
    }
    out
}

/// Une partie en deux, au début de plan le plus proche de son milieu ; None sans plan où couper.
fn halve(spans: &[(f64, f64)], (a, b): (i64, i64)) -> Option<[(i64, i64); 2]> {
    let mid = (a + b) as f64 / 2.0;
    spans
        .iter()
        .map(|s| s.0.ceil() as i64)
        .filter(|&f| f > a && f < b)
        .min_by(|x, y| (*x as f64 - mid).abs().total_cmp(&(*y as f64 - mid).abs()))
        .map(|c| [(a, c), (c, b)])
}

/// Empreinte du montage et du code du rendu : une partie déjà rendue ne resert qu'au même.
fn parts_key(m: &Montage, mode: &str, k: i64, preset: Option<&str>) -> String {
    let mut h = Sha1::new();
    let sources: [Option<PathBuf>; 6] = [
        Some(paths::data().join("edit.json")),
        Some(paths::data().join("face.json")),
        None, // code du rendu : ce fichier-ci
        Some(hf_audio()),
        Some(paths::work().join("son.json")),
        Some(hf_look()),
    ];
    for p in sources {
        match p {
            None => h.update(include_bytes!("render_hyperframes.rs")),
            Some(p) => match fs::read(&p) {
                Ok(b) => h.update(&b),
                Err(_) => h.update(b"-"),
            },
        }
    }
    let (rush, key) = m.rush();
    let size = fs::metadata(&rush).map(|m| m.len()).unwrap_or(0);
    h.update(format!("{mode} {k} {} ('{}', '{key}') {size}", preset.unwrap_or("None"), rush.display()).as_bytes());
    hex::encode(h.finalize())[..12].to_string()
}

/// Parties mises bout à bout : image copiée telle quelle ; son de chaque partie décodé et
/// ramené à la durée exacte de la partie (sans le blanc d'encodage de son début, qui décalerait
/// la suite), mis bout à bout puis encodé d'un seul tenant.
fn join_parts(paths_: &[PathBuf], lengths: &[f64], out: &Path) -> i32 {
    let ff = paths::ffmpeg();
    let tmp = PathBuf::from(format!("{}.assemblage", out.display()));
    util::mkdirs(&tmp);
    let code = (|| {
        let mut wavs = vec![];
        for (i, (path, length)) in paths_.iter().zip(lengths).enumerate() {
            let wav = tmp.join(format!("{i:03}.wav"));
            let af = format!("apad,atrim=duration={length:.6}");
            let code = procs::call(
                ff,
                [
                    "-v".as_ref(), "error".as_ref(), "-y".as_ref(), "-i".as_ref(), path.as_os_str(), "-vn".as_ref(),
                    "-ac".as_ref(), "2".as_ref(), "-ar".as_ref(), "48000".as_ref(), "-af".as_ref(), af.as_ref(),
                    "-c:a".as_ref(), "pcm_s16le".as_ref(), wav.as_os_str(),
                ] as [&std::ffi::OsStr; 15],
            );
            if code != 0 {
                return 1;
            }
            wavs.push(wav);
        }
        for (name, items) in [("images.txt", paths_), ("son.txt", &wavs[..])] {
            let text: String = items
                .iter()
                // apostrophe du chemin échappée comme dans join_rushes (liste de concat de ffmpeg)
                .map(|p| format!("file '{}'\n", std::path::absolute(p).unwrap_or_else(|_| p.clone()).display().to_string().replace('\'', "'\\''")))
                .collect();
            if fs::write(tmp.join(name), text).is_err() {
                return 1;
            }
        }
        let images = tmp.join("images.txt");
        let son = tmp.join("son.txt");
        procs::call(
            ff,
            [
                "-v".as_ref(), "error".as_ref(), "-y".as_ref(), "-f".as_ref(), "concat".as_ref(), "-safe".as_ref(), "0".as_ref(),
                "-i".as_ref(), images.as_os_str(), "-f".as_ref(), "concat".as_ref(), "-safe".as_ref(), "0".as_ref(), "-i".as_ref(),
                son.as_os_str(), "-map".as_ref(), "0:v".as_ref(), "-map".as_ref(), "1:a".as_ref(), "-c:v".as_ref(), "copy".as_ref(),
                "-c:a".as_ref(), "aac".as_ref(), "-b:a".as_ref(), "192k".as_ref(), "-movflags".as_ref(), "+faststart".as_ref(),
                out.as_os_str(),
            ] as [&std::ffi::OsStr; 28],
        )
    })();
    let _ = fs::remove_dir_all(&tmp);
    code
}

fn fail(msg: impl std::fmt::Display) -> i32 {
    eprintln!("{msg}");
    1
}

pub fn main(args: &[String]) -> i32 {
    let (Some(mode), Some(out)) = (args.first(), args.get(1)) else {
        return fail("Usage : auto-montage render_hyperframes apercu|final <sortie.mp4>");
    };
    let hd = mode == "final";
    let mut m = match Montage::load(hd) {
        Ok(m) => m,
        Err(e) => return fail(e),
    };
    let preset = if hd { hd_preset(m.width, m.height) } else { None };
    let k = if hd && preset.is_none() { 2 } else { 1 };
    let fps = m.fps;
    let total = round_to(m.frames as f64 / fps, 2);
    for cache in place::extract_caches() {
        // images extraites gardées par un rendu précédent
        let _ = fs::remove_dir_all(cache);
    }
    // Pixels de l'image rendue et des images extraites de la source (4K : le double de chaque côté).
    let pixels = (m.width * m.height * if hd { 4 } else { 1 }) as f64;
    // Rendu précédent arrêté en route : ses navigateurs restés ouverts (ils ont leur propre session).
    let left = procs::kill_marked(procs::RENDER_MARK);
    if left > 0 {
        say(&format!("Programmes d'un rendu précédent encore ouverts arrêtés : {left}"));
        std::thread::sleep(Duration::from_secs(2)); // leur mémoire rendue avant de compter la libre
    }
    // Navigateurs et plans par partie : autant de navigateurs que 80 % de la mémoire libre le
    // permet, un par cœur au plus, chacun gardant tous les plans de sa page (memoire::render_plan,
    // d'après les mesures des rendus précédents sur cette machine) ; moins si le disque est trop
    // plein pour leur mémoire partagée.
    let spans = clip_spans(&m.edit);
    let wanted = parts_of(m.frames, fps, None).iter().map(|&(a, b)| clips_in(&spans, a, b)).max().unwrap_or(1);
    let info = VideoInfo { duration: 0.0, source: (m.width, m.height), work: (m.width, m.height), montage: total, parts: 1, workers: None, clips: None };
    let (mut workers, most_clips) = memoire::render_plan(hd, &info, None, wanted);
    let info = VideoInfo { clips: Some(most_clips as f64), ..info };
    let free = memoire::disk_free(paths::root()).unwrap_or(0) as f64;
    let mut longest = disk_part(fps, pixels, free, workers, total);
    while longest.map(|l| l < MIN_PART).unwrap_or(false) && workers > 1 {
        workers -= 1;
        longest = disk_part(fps, pixels, free, workers, total);
    }
    if longest.map(|l| l < MIN_PART).unwrap_or(false) {
        let need = (MIN_PART * fps * FRAME_BYTES + BROWSER_BYTES + total * OUT_BYTES) * pixels / DISK_SHARE;
        return fail(format!(
            "Pas assez de place sur le disque pour le rendu : {} libres, il en faut au moins {}. De la place peut être \
             libérée en bas de la page « Rushes et montages » (« Libérer de la place »).",
            memoire::go(free as i64),
            memoire::go(need as i64)
        ));
    }
    let mut why = if longest.map(|l| l != 0.0).unwrap_or(false) {
        format!(" (place sur le disque : {} libres)", memoire::go(free as i64))
    } else {
        String::new()
    };
    if let Some(by_memory) = memory_part(fps, pixels, hd, &info, workers) {
        if longest.map(|l| by_memory < l).unwrap_or(true) {
            longest = Some(by_memory);
            why = " (mémoire des navigateurs en 4K)".into();
        }
    }
    let mut parts = split_by_clips(parts_of(m.frames, fps, longest), &spans, most_clips);
    if parts.len() > parts_of(m.frames, fps, longest).len() && why.is_empty() {
        why = format!(" (mémoire : {most_clips} plans au plus par partie)");
    }
    if parts.len() > 1 && 2.0 * total * OUT_BYTES * pixels > DISK_SHARE * free {
        say(&format!(
            "Attention : disque presque plein ({} libres), l'assemblage des parties à la fin pourrait manquer de place (environ {}).",
            memoire::go(free as i64),
            memoire::go((2.0 * total * OUT_BYTES * pixels) as i64)
        ));
    }
    let avail = memoire::available().0;
    say(&format!(
        "Navigateurs du rendu : {workers}{}",
        match avail {
            Some(a) if a != 0 => format!(" ({} de mémoire libre)", memoire::go(a)),
            _ => String::new(),
        }
    ));
    // Plans de la plus grande page : comptent dans la mesure de la mémoire du rendu (local_server).
    say(&format!("Plans par partie du rendu : {}", parts.iter().map(|&(a, b)| clips_in(&spans, a, b)).max().unwrap_or(0)));
    // Long montage : rendu par parties d'environ PART_SECONDS, chacune dans sa propre page avec ses
    // seuls plans (mémoire de la plus longue partie, pas du montage entier), puis mises bout à bout.
    // Une partie finie est gardée (work/rendu_parties/) : un rendu interrompu reprend aux suivantes.
    let key = parts_key(&m, mode, k, preset);
    let pdir = parts_dir();
    util::mkdirs(&pdir);
    for name in util::list_dir(&pdir) {
        // parties d'un autre montage, dossiers d'un rendu interrompu
        if !name.starts_with(&key) {
            util::remove_all(pdir.join(name));
        }
    }
    if parts.len() > 1 {
        say(&format!(
            "Rendu en {} parties d'environ {} s{}",
            parts.len(),
            round_int(total / parts.len() as f64),
            why
        ));
    }
    let mut env = match tools(hd) {
        Ok(e) => e,
        Err(e) => return fail(e),
    };
    report("Rendu de la vidéo", 0.0, total);
    let mut done_paths = vec![];
    // Partie arrêtée par la surveillance de la mémoire : refaite avec moitié moins de navigateurs,
    // puis coupée en deux ; une partie d'un seul plan avec un navigateur est rendue sans surveillance.
    let mut i = 0;
    while i < parts.len() {
        let (f0, f1) = parts[i];
        // Nom de la partie : ses bornes (le découpage dépend aussi du disque libre).
        let path = if parts.len() == 1 { PathBuf::from(out) } else { pdir.join(format!("{key}_{f0:07}-{f1:07}.mp4")) };
        let part_len = (f1 - f0) as f64 / fps;
        if parts.len() > 1 && path.exists() {
            say(&format!("Partie {} déjà rendue : gardée", i + 1));
        } else {
            let _ = fs::remove_dir_all(dir());
            util::mkdirs(dir().join("assets"));
            let _ = fs::write(dir().join("hyperframes.json"), pyjson::dumps(&json!({"paths": {"assets": "assets"}})));
            if let Err(e) = build(&mut m, k, Some((f0, f1))) {
                return fail(e);
            }
            match limiter_exe() {
                Ok(p) => {
                    env.retain(|(k, _)| k != "HYPERFRAMES_FFMPEG_PATH");
                    env.push(("HYPERFRAMES_FFMPEG_PATH".into(), paths::display(&p)));
                }
                Err(e) => say(&format!("Attention : {e} ; extractions d'images sans limite")),
            }
            let tmp = if parts.len() == 1 {
                path.clone()
            } else {
                let s = path.to_string_lossy();
                PathBuf::from(format!("{}.part.mp4", &s[..s.len() - 4]))
            };
            let abs = std::path::absolute(&tmp).unwrap_or_else(|_| tmp.clone());
            let mut cmd = vec![
                paths::display(&paths::node()),
                paths::display(&node_modules().join("hyperframes").join("dist").join("cli.js")),
                "render".into(),
                paths::display(&dir()),
                "-o".into(),
                paths::display(&abs),
                "--workers".into(),
                workers.to_string(),
                "--crf".into(),
                if hd { "16" } else { "20" }.into(),
            ];
            if let Some(p) = preset {
                cmd.extend(["--resolution".into(), p.to_string()]);
            }
            if parts.len() > 1 {
                say(&format!("Partie {} sur {}", i + 1, parts.len()));
            }
            // Vidéo encodée par la carte graphique si elle marche (gpu.rs ; HyperFrames choisit
            // son encodeur matériel) ; échec : partie refaite par le processeur.
            let enc = crate::gpu::h264();
            let watch = workers > 1 || halve(&spans, (f0, f1)).is_some();
            let mut code = run_hyperframes(&[cmd.clone(), if enc.is_gpu() { vec!["--gpu".into()] } else { vec![] }].concat(), &env, f0 as f64 / fps, part_len, total, watch);
            if code != 0 && code != LOW_MEMORY && enc.is_gpu() {
                crate::gpu::give_up(&enc);
                code = run_hyperframes(&cmd, &env, f0 as f64 / fps, part_len, total, watch);
            }
            if code == LOW_MEMORY {
                let _ = fs::remove_file(&tmp);
                if workers > 1 {
                    workers = (workers / 2).max(1);
                    say(&format!("Mémoire presque épuisée : partie {} refaite avec {workers} navigateur{}", i + 1, if workers > 1 { "s" } else { "" }));
                } else if let Some(two) = halve(&spans, (f0, f1)) {
                    parts.splice(i..=i, two);
                    say(&format!("Mémoire presque épuisée : partie {} refaite en deux, rendu en {} parties", i + 1, parts.len()));
                }
                continue;
            }
            if code != 0 || !tmp.exists() {
                let left = memoire::disk_free(paths::root()).unwrap_or(u64::MAX);
                return fail(format!(
                    "Échec du rendu HyperFrames (code {code}){}",
                    if left < 2 * (1u64 << 30) { format!(" : disque presque plein ({} libres)", memoire::go(left as i64)) } else { String::new() }
                ));
            }
            if tmp != path {
                if let Err(e) = fs::rename(&tmp, &path) {
                    return fail(format!("Échec du rendu HyperFrames : {e}"));
                }
            }
        }
        done_paths.push(path);
        report_every("Rendu de la vidéo", round_to(f1 as f64 / fps, 2), total, 0.0);
        i += 1;
    }
    if parts.len() > 1 {
        say("Assemblage des parties");
        let lengths: Vec<f64> = parts.iter().map(|&(f0, f1)| (f1 - f0) as f64 / fps).collect();
        if join_parts(&done_paths, &lengths, Path::new(out)) != 0 || !Path::new(out).exists() {
            return fail("Échec de l'assemblage des parties du rendu");
        }
        for name in util::list_dir(&pdir) {
            if name.starts_with(&key) {
                util::remove_all(pdir.join(name));
            }
        }
    }
    // Liens vers le rush (et copie du programme pour ffmpeg_limite) : inutiles ensuite.
    let _ = fs::remove_dir_all(dir().join("assets"));
    let _ = fs::remove_dir_all(dir().join("bin"));
    say(&format!("Rendu HyperFrames : {out}"));
    0
}

const PAGE: &str = r##"<!doctype html>
<html lang="fr">
<head>
<meta charset="UTF-8" />
<meta name="viewport" content="width=%OUTW%, height=%OUTH%" />
<title>Montage</title>
<script src="assets/gsap.min.js"></script>
<script src="assets/hf_look.js"></script>
<style>
@font-face { font-family: "AM Poppins"; src: url("%POPPINS%"); font-weight: 900; }
@font-face { font-family: "AM Sous-titres"; src: url("%CAPFONT%"); font-weight: %CAPWEIGHT%; }
html, body { margin: 0; background: black; }
#root { position: relative; width: %OUTW%px; height: %OUTH%px; overflow: hidden; background: black; }
#frame { position: absolute; left: 0; top: 0; width: %W%px; height: %H%px; overflow: hidden; transform: scale(%K%); transform-origin: 0 0; }
.fill { position: absolute; inset: 0; display: flex; flex-direction: column; }
.clip { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
#typed { justify-content: center; align-items: center; padding-top: %TYPEDTOP%px; box-sizing: border-box; }
#typed div { font-family: "AM Poppins"; font-weight: 900; font-size: 92px; line-height: 1.02; color: white;
  text-align: center; white-space: pre-line; text-shadow: 0 3px 12px rgba(0,0,0,0.35), 0 1px 3px rgba(0,0,0,0.3); padding: 0 70px; }
#chips { align-items: center; padding-top: 170px; box-sizing: border-box; }
#chips div { font-family: "AM Poppins"; font-weight: 900; font-size: 50px; line-height: 1; color: #1d1d1b; background: white;
  padding: 22px 36px; border-radius: 999px; box-shadow: 0 8px 24px rgba(0,0,0,0.25); }
#caps { align-items: center; top: %CAPTOP%px; }
#caps > div { display: flex; flex-direction: column; align-items: center; gap: 4px; }
#flash { background: white; }
#look { background: black; }
</style>
</head>
<body>
<div id="root" data-composition-id="main" data-start="0" data-duration="%DUR%" data-fps="%FPS%" data-width="%OUTW%" data-height="%OUTH%">
<div id="frame">
<svg width="0" height="0" style="position:absolute"><defs id="amdefs"></defs></svg>
<div id="look" class="fill">
%VIDEOS%
</div>
<div id="trov" class="fill" style="opacity:0"></div>
<div id="vign" class="fill"></div>
<div id="fxov" class="fill"></div>
%AUDIOS%
<div id="typed" class="fill"></div>
<div id="chips" class="fill"></div>
<div id="caps" class="fill"></div>
<div id="flash" class="fill" style="opacity:0"></div>
</div>
</div>
<script>
// Chaque image est dessinée d'après son numéro (timeline GSAP en pause,
// déplacée image par image par HyperFrames).
const D = %DATA%;
const FPS = D.fps, HL = 1.2;
const inRange = (t, r) => t >= r.start && t < r.end;
const clamp = (x, a, b) => Math.min(b, Math.max(a, x));
const ease = (x) => { x = clamp(x, 0, 1); return x < 0.5 ? 2 * x * x : 1 - Math.pow(-2 * x + 2, 2) / 2; };
const spring = (frame, damping, mass, from = 0, to = 1) => {
  if (frame < 0) return from;
  const t = frame / FPS, k = 100, zeta = damping / (2 * Math.sqrt(k * mass)), w0 = Math.sqrt(k / mass);
  let x;
  if (zeta < 1) { const w1 = w0 * Math.sqrt(1 - zeta * zeta); x = Math.exp(-zeta * w0 * t) * (zeta * w0 / w1 * Math.sin(w1 * t) + Math.cos(w1 * t)); }
  else x = Math.exp(-w0 * t) * (1 + w0 * t);
  return from + (to - from) * (1 - x);
};
const sourceTime = (f) => {
  const c = D.clips.find((c) => f >= c.from && f < c.from + c.durationInFrames) || D.clips[D.clips.length - 1];
  return (c.trimBefore + f - c.from) / FPS;
};
const faceAt = (t, person) => {
  const F = D.face, i = Math.min(F.length - 2, Math.max(0, Math.floor(t / 0.5))), a = F[i], b = F[i + 1];
  const k = clamp((t - a.t) / ((b.t - a.t) || 1), 0, 1);
  const focused = person && focusFrame(a, b, k, person);
  if (focused) return focused;
  // s : zoom maximal qui garde tous les visages retenus dans l'image (detect_face).
  return { x: a.x + (b.x - a.x) * k, y: a.y + (b.y - a.y) * k, s: Math.min(a.s, b.s) };
};
// Moment cadré sur une personne (page des moments) : son visage, à la taille d'un plan rapproché
// (visage sur FOCUS_H de la hauteur, zoom jusqu'à ×2,5 au plus), centré quand c'est possible.
// Personne dans un cadre (écran partagé, incrustation : x0, y0, x1, y1 de detect_face) : zoom
// d'au moins la taille du cadre (×PANEL_MAX au plus) et image visible gardée dans le cadre.
// Même règle dans la page des moments (faceInClip de selection/page.html).
const FOCUS_H = 0.22, PANEL_MAX = 4;
const focusFrame = (a, b, k, person) => {
  const fa = (a.faces || []).find((f) => f[0] === person), fb = (b.faces || []).find((f) => f[0] === person);
  const f = fa && fb ? fa.slice(0, 5).map((v, i) => i ? v + (fb[i] - v) * k : v) : fa || fb;
  if (!f) return null;
  const [, x, y, w, h] = f, near = (k < 0.5 ? fa : fb) || fa || fb;
  const panel = near.length > 8 ? near.slice(5, 9) : null;
  // Zoom maximal : le visage, avec un peu d'air, tient dans l'image visible (centrée sur lui).
  let s = clamp(Math.min(1 / (1.4 * w), 1 / (1.4 * h)), 1, 2.5), base = clamp(FOCUS_H / h, 1, 2.5);
  if (panel) {
    const need = Math.min(PANEL_MAX, Math.max(1 / (panel[2] - panel[0]), 1 / (panel[3] - panel[1])));
    s = Math.max(s, need);
    base = Math.max(base, need);
  }
  return { x, y, s, base, center: true, panel };
};
// Origine du zoom qui met le point c au centre de l'image visible, bornée aux bords de l'image,
// ou à ceux du cadre [lo, hi] de la personne (l'image visible, 1/scale, reste dedans).
const centeredOrigin = (c, scale, lo = 0, hi = 1) => {
  if (scale - 1 < 1e-3) return c;
  const v = 1 / scale, left = hi - lo >= v ? clamp(c - v / 2, lo, hi - v) : (lo + hi - v) / 2;
  return clamp(left / (1 - v), 0, 1);
};
// Ombre : intensité (0-100, plus ou moins sombre), diffusion du flou et décalage (ox, oy), en px sur 1080.
// Même formule dans la page des moments (captionShadow de selection/page.html).
const captionShadow = (shadow = 50, blur = 14, ox = 0, oy = 0) => {
  if (shadow <= 0) return "none";
  const s = shadow / 100, at = (b, a) => `${ox}px ${oy}px ${b}px rgba(0,0,0,${Math.min(1, a).toFixed(3)})`;
  const layers = [at(blur, 1.2 * s), at(blur / 2, 0.8 * s)];
  if (s > 0.5) layers.push(at(blur / 4, 1.6 * (s - 0.5)));
  return layers.join(", ");
};
const C = D.cap;
const capStyle = `font-family:"AM Sous-titres";font-weight:${C.weight};color:${C.color};text-transform:${C.uppercase ? "uppercase" : "none"};` +
  `text-align:center;line-height:1.05;white-space:nowrap;text-shadow:${captionShadow(C.shadow ?? 50, C.blur ?? 14, C.ox ?? 0, C.oy ?? 0)};`;
const el = (id) => document.getElementById(id);
const last = {};
// Contenu de l'élément id remplacé par les nœuds de make() quand key change. Pas de HTML fait des
// données : textes par textContent, couleurs et tailles par style (une valeur invalide est ignorée).
const put = (id, key, make) => { if (last[id] !== key) { last[id] = key; el(id).replaceChildren(...(make ? [make()] : [])); } };
const node = (tag, css, text) => {
  const n = document.createElement(tag);
  if (css) n.style.cssText = css;
  if (text) n.textContent = text;
  return n;
};

function draw(frame) {
  const t = frame / FPS, H = D.hab;
  // Zooms automatiques, zoom placé sur un moment, secousse.
  const zoom = D.zooms.find((z) => inRange(t, z)) || D.zooms[D.zooms.length - 1] || { type: "in", start: 0, end: 1 };
  let scale = zoom.type === "in" ? 1 + 0.12 * H.autoZoom * ease((t - zoom.start) / Math.max(0.01, zoom.end - zoom.start)) : 1 + 0.22 * H.autoZoom;
  // Zoom libre (« key ») : d'un cadre à l'autre entre les instants a et b du rush, sur tout le
  // moment ; il passe avant les autres zooms et le plafond des visages (cadre choisi à la main).
  const kz = D.zoomFx.find((z) => z.type === "key" && inRange(t, z));
  const m = kz ? null : D.zoomFx.find((z) => z.type !== "key" && inRange(t, z));
  if (m) { const p = ease((t - m.start) / Math.max(0.01, m.anim)); scale = m.type === "in" ? 1 + m.force * p : m.type === "punch" ? 1 + m.force : 1 + m.force * (1 - p); }
  const sh = D.shakes.find((x) => inRange(t, x));
  let dx = 0, dy = 0;
  if (sh) {
    const damp = 1 - (t - sh.start) / Math.max(0.01, sh.end - sh.start);
    dx = 40 * sh.force * damp * (Math.sin(t * 71) + 0.5 * Math.sin(t * 37));
    dy = 40 * sh.force * damp * (Math.cos(t * 59) + 0.5 * Math.sin(t * 43));
  }
  let zt, zo;
  if (kz) {  // même calcul que keyZoomAt de la page des moments
    const p = ease((sourceTime(frame) - kz.a) / Math.max(0.05, kz.b - kz.a)), A = kz.from, B = kz.to;
    const s = A.s + (B.s - A.s) * p, x = A.x + (B.x - A.x) * p, y = A.y + (B.y - A.y) * p;
    zt = `translate(${dx}px, ${dy}px) scale(${s})`;
    zo = `${centeredOrigin(x, s) * 100}% ${centeredOrigin(y, s) * 100}%`;
  } else {
  const fo = D.focus.find((x) => inRange(t, x));
  const f = faceAt(sourceTime(frame), fo && fo.person);
  // Personne ne sort de l'image (zooms automatiques et placés) ; cadrage sur une personne :
  // zoom de base du plan rapproché, plafonné pour garder son visage entier.
  scale = Math.min(scale * (f.base || 1), f.s);
  zt = `translate(${dx}px, ${dy}px) scale(${scale})`;
  const P = f.panel || [0, 0, 1, 1];
  zo = f.center ? `${centeredOrigin(f.x, scale, P[0], P[2]) * 100}% ${centeredOrigin(f.y, scale, P[1], P[3]) * 100}%`
    : `${f.x * 100}% ${f.y * 100}%`;
  }
  // Cadrage du plan en cours ; pendant une transition, le plan sortant garde le même.
  const ci = D.clips.findIndex((c) => frame >= c.from && frame < c.from + c.durationInFrames);
  const tr = D.transitions.find((x) => frame >= x.at && frame < x.at + x.frames);
  for (const i of tr ? [tr.clip - 1, tr.clip] : [ci]) {
    const zb = el(`zb${i}`);
    if (zb) { zb.style.transform = zt; zb.style.transformOrigin = zo; }
  }
  look(frame, t, tr);

  // Texte tapé.
  const ty = D.typed.find((x) => inRange(t, x));
  if (ty) {
    const chars = ty.text.replace(/\n/g, "").length;
    const shown = Math.floor(clamp((t - ty.start) / Math.max(1e-6, ty.typeDuration), 0, 1) * chars);
    let count = 0, visible = "";
    for (const ch of ty.text) { if (ch === "\n") { visible += ch; continue; } if (count >= shown) break; visible += ch; count++; }
    const cursor = Math.floor(t * 3) % 2 === 0 || shown < chars;
    el("typed").style.opacity = clamp((ty.end - t) / 0.15, 0, 1);
    put("typed", JSON.stringify([visible, cursor]), () => {
      const d = node("div", "", visible);
      d.append(node("span", `opacity:${cursor ? 1 : 0}`, "|"));
      return d;
    });
  } else put("typed", "");

  // Pastille.
  const chip = D.chips.find((c) => inRange(t, c));
  if (chip) {
    const s = spring(frame - Math.round(chip.start * FPS), 11, 0.6) * clamp((chip.end - t) / 0.15, 0, 1);
    put("chips", JSON.stringify([s, chip.text]), () => node("div", `transform:scale(${s})`, chip.text));
  } else put("chips", "");

  // Sous-titres (pas pendant un texte tapé).
  const cap = ty ? null : D.captions.find((c) => inRange(t, c));
  if (cap) {
    let cur = -1;
    if (H.karaoke) cap.times.forEach((x, i) => { if (t >= x) cur = i; });
    const pop = H.pop ? spring(frame - Math.round(cap.start * FPS), 10, 0.5, 0.75, 1) : 1;
    put("caps", JSON.stringify([cap.start, cap.words, cur, pop]), () => {
      const box = node("div", `transform:scale(${pop})`);
      for (const [a, b, k] of cap.lines) {
        const line = node("div", `${capStyle}font-size:${C.size * k * cap.scale}px`);
        cap.words.slice(a, b).forEach((w, j) => {
          const i = a + j, c = i === cur ? H.karaokeColor : cap.colors[i];
          const sp = node("span", "", `${j ? " " : ""}${w}`);
          if (c) sp.style.color = c;
          if (cap.colors[i]) sp.style.fontSize = `${HL}em`;
          line.append(sp);
        });
        box.append(line);
      }
      return box;
    });
    // Position choisie (posX, posY : décalage en % de la moitié de l'image), le sous-titre restant à
    // 40 px au moins des bords ; lignes nombreuses ou grandes : remonté pour finir à 40 px du bas.
    const box = el("caps").firstElementChild, top = el("caps").offsetTop;
    const room = Math.max(0, (%W% - box.offsetWidth) / 2 - 40);
    const x = Math.max(-room, Math.min(room, ((C.posX || 0) / 100) * (%W% / 2)));
    const y = Math.max(40, Math.min(%H% - 40 - box.offsetHeight, top + ((C.posY || 0) / 100) * (%H% / 2)));
    el("caps").style.translate = x || y !== top ? `${x}px ${y - top}px` : "";
  } else { put("caps", ""); el("caps").style.translate = ""; }

  // Flash blanc.
  const fl = D.flashes.find((x) => inRange(t, x));
  const p = fl ? (t - fl.start) / Math.max(0.01, fl.end - fl.start) : 0;
  el("flash").style.opacity = fl ? 0.95 * (1 - p) * Math.min(1, p * 8 + 0.4) : 0;
}

// Couleurs de tout le montage, effet d'image du moment, transition entre deux moments
// (selection/hf_look.js, les mêmes calculs que l'aperçu de la page des moments).
const COLOR = HFLook.colorFilter(D.couleur || {});
el("vign").style.background = COLOR.vignette || "";
let styled = [];
function look(frame, t, tr) {
  for (const id of styled) { const b = el(id); if (b) HFLook.applyStyle(b, null); }
  styled = [];
  const ov = el("trov");
  ov.style.opacity = 0;
  if (tr) {
    const S = HFLook.transitionStyles(tr.kind, (frame - tr.at + 1) / (tr.frames + 1), %W%, %H%);
    for (const [i, st] of [[tr.clip - 1, S.out], [tr.clip, S.in]]) {
      const b = el(`box${i}`);
      if (b) { HFLook.applyStyle(b, st); styled.push(`box${i}`); }
    }
    if (S.over) { ov.style.background = S.over.background; ov.style.opacity = S.over.opacity; }
  }
  const lk = D.looks.find((x) => inRange(t, x));
  const L = lk ? HFLook.effectLayers(lk.effect, lk.force, t - lk.start, lk.end - lk.start, %W%, frame) : { css: "", svg: "", over: "", flip: null };
  put("amdefs", COLOR.svg + L.svg);
  const lookEl = el("look");
  lookEl.style.filter = [COLOR.css, L.css].filter(Boolean).join(" ");
  lookEl.style.transform = L.flip ? `scale(${L.flip[0]}, ${L.flip[1]})` : "";
  el("fxov").style.cssText = L.over || "";
}

// Le numéro d'image suit le temps de la timeline (propriété d'un objet animée par GSAP).
// Partie du montage rendue seule : son temps commence à l'image OFF du montage.
const OFF = %OFF%;
let shownFrame = -1;
const clock = {
  get t() { return (shownFrame - OFF) / FPS; },
  set t(v) { const f = Math.min(D.frames - 1, Math.max(0, OFF + Math.round(v * FPS))); if (f !== shownFrame) { shownFrame = f; draw(f); } },
};
const tl = gsap.timeline({ paused: true });
tl.fromTo(clock, { t: 0 }, { t: %DUR%, duration: %DUR%, ease: "none", immediateRender: true }, 0);
clock.t = 0;
window.__timelines = window.__timelines || {};
window.__timelines["main"] = tl;
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r6_like_python() {
        assert_eq!(r6(0.0), "0");
        assert_eq!(r6(10.0), "10");
        assert_eq!(r6(1.0 / 3.0), "0.333333");
        assert_eq!(r6(0.25 / 30.0), "0.008333");
        assert_eq!(r6(14.7), "14.7");
    }

    #[test]
    fn parts_like_python() {
        assert_eq!(parts_of(441, 30.0, None), vec![(0, 441)]);
        // 10 min à 30 i/s : 5 parties de 2 min.
        assert_eq!(parts_of(18000, 30.0, None).len(), 5);
        // 3 min : 1,5 fois la durée d'une partie au plus, une seule.
        assert_eq!(parts_of(5400, 30.0, None), vec![(0, 5400)]);
        // Disque : parties de 50 s au plus.
        let p = parts_of(5400, 30.0, Some(50.0));
        assert_eq!(p, vec![(0, 1350), (1350, 2700), (2700, 4050), (4050, 5400)]);
        assert_eq!(disk_part(30.0, 1080.0 * 1920.0, 1e15, 4, 60.0), None);
    }

    #[test]
    fn parts_by_clips() {
        // 60 plans de 3 s (90 images), le 10e prolongé de 15 images par une transition.
        let edit = json!({
            "clips": (0..60).map(|i| json!({"from": i * 90, "durationInFrames": 90})).collect::<Vec<_>>(),
            "transitions": [{"clip": 10, "frames": 15}],
        });
        let spans = clip_spans(&edit);
        assert_eq!(spans[9], (810.0, 915.0));
        assert_eq!(clips_in(&spans, 0, 5400), 60);
        assert_eq!(clips_in(&spans, 900, 1800), 11); // le 10e passe encore sous le 11e
        let p = split_by_clips(vec![(0, 5400)], &spans, 36);
        assert_eq!(p, vec![(0, 3240), (3240, 5400)]);
        assert!(p.iter().all(|&(a, b)| clips_in(&spans, a, b) <= 36));
        // Assez de mémoire : parties gardées telles quelles.
        assert_eq!(split_by_clips(vec![(0, 2700), (2700, 5400)], &spans, 60), vec![(0, 2700), (2700, 5400)]);
        // Un seul plan par partie au plus : coupé à chaque plan, même pendant la transition.
        assert_eq!(split_by_clips(vec![(0, 270)], &spans, 1), vec![(0, 90), (90, 180), (180, 270)]);
        assert_eq!(halve(&spans, (0, 5400)), Some([(0, 2700), (2700, 5400)]));
        assert_eq!(halve(&spans, (90, 180)), None);
    }

    #[test]
    fn page_keys_present() {
        for k in ["%OUTW%", "%OUTH%", "%W%", "%H%", "%K%", "%CAPTOP%", "%TYPEDTOP%", "%FPS%", "%DUR%", "%OFF%", "%VIDEOS%", "%AUDIOS%", "%POPPINS%", "%CAPFONT%", "%CAPWEIGHT%", "%DATA%"] {
            assert!(PAGE.contains(k), "{k}");
        }
    }

    /// Page écrite pour un montage préparé (AM_ROOT), pour l'examiner sans lancer le rendu :
    /// AM_ROOT=… cargo test page_seule -- --ignored  (avec AM_PART=début,fin pour une partie).
    #[test]
    #[ignore]
    fn page_seule() {
        let mut m = Montage::load(std::env::var("AM_HD").is_ok()).unwrap();
        let part = std::env::var("AM_PART").ok().map(|p| {
            let (a, b) = p.split_once(',').unwrap();
            (a.parse().unwrap(), b.parse().unwrap())
        });
        let _ = fs::remove_dir_all(dir());
        util::mkdirs(dir().join("assets"));
        let f = m.frames;
        build(&mut m, 1, Some(part.unwrap_or((0, f)))).unwrap();
    }

    /// Assemblage de deux parties (la vidéo rendue deux fois) : AM_ROOT=… cargo test assemblage -- --ignored
    #[test]
    #[ignore]
    fn assemblage() {
        let v = paths::out().join("montage_apercu.mp4");
        let out = paths::out().join("essai_assemblage.mp4");
        assert_eq!(join_parts(&[v.clone(), v], &[14.7, 14.7], &out), 0);
        let d = crate::progress::duration_of(&procs::ffmpeg_info(&out));
        assert!((d - 29.4).abs() < 0.05, "{d}");
        let _ = fs::remove_file(out);
    }
}
