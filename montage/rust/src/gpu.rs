//! Carte graphique pour encoder en H.264 (ffmpeg), avec repli sur le processeur.
//!
//! Encodeurs matériels essayés (dans l'ordre, ceux de ce système) : h264_videotoolbox (macOS),
//! h264_nvenc, h264_qsv, h264_amf (Windows), h264_nvenc, h264_vaapi, h264_qsv (Linux). Un encodeur
//! est retenu s'il est dans le ffmpeg utilisé (`ffmpeg -encoders`) et s'il encode vraiment sur
//! cette machine (essai de quelques images, avec les réglages de qualité et de débit utilisés
//! ensuite). Résultat gardé dans work/gpu.json, refait quand le ffmpeg change (chemin, taille,
//! date). Sinon : libx264 (processeur). `AM_GPU=0` : toujours le processeur (dépannage).
//!
//! Qualité comparable : le crf de libx264 devient le cq de NVENC, le global_quality de QSV, le qp
//! de VAAPI et d'AMF, ou une qualité de 1 à 100 pour VideoToolbox (`quality_value`). Avec la carte
//! graphique, la vidéo lue est aussi décodée par elle si possible (`-hwaccel auto`, repli sur le
//! processeur par ffmpeg) : seulement pour les encodages (version de travail, source 4K, exports),
//! jamais pour les analyses (visages, son, style), qui lisent leurs vidéos avec le processeur
//! pour des résultats identiques d'une machine à l'autre. Un encodage qui échoue sur la carte
//! graphique est refait par le processeur (`encode`), et la carte n'est plus essayée tant que le
//! ffmpeg ne change pas (effacer work/gpu.json pour réessayer).
//!
//! Usage : auto-montage gpu     encodeur retenu (essai refait)

use crate::{paths, progress, util};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Encodeur du processeur.
pub const CPU: &str = "libx264";
/// Carte graphique des encodeurs VAAPI (Linux).
const VAAPI_DEVICE: &str = "/dev/dri/renderD128";
/// Durée maximale de l'essai d'un encodeur (un pilote bloqué ne doit pas tout arrêter).
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// Encodeurs matériels essayés pour ce système, du préféré au dernier.
pub fn candidates() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["h264_videotoolbox"]
    } else if cfg!(windows) {
        &["h264_nvenc", "h264_qsv", "h264_amf"]
    } else {
        &["h264_nvenc", "h264_vaapi", "h264_qsv"]
    }
}

/// Réglage du débit : qualité constante (crf de libx264) ou débit visé (kbit/s) et plafond.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rate {
    Crf(u32),
    Bitrate { kbps: i64, max_kbps: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoder {
    pub name: String,
}

impl Encoder {
    pub fn cpu() -> Encoder {
        Encoder { name: CPU.into() }
    }

    pub fn is_gpu(&self) -> bool {
        self.name != CPU
    }

    /// « carte graphique (h264_nvenc) » ou « processeur ».
    pub fn label(&self) -> String {
        if self.is_gpu() {
            format!("carte graphique ({})", self.name)
        } else {
            "processeur".into()
        }
    }

    /// Options à placer juste avant le « -i » de la vidéo lue (décodage par la carte graphique).
    pub fn input_args(&self) -> Vec<String> {
        let mut a: Vec<String> = vec![];
        if self.name == "h264_vaapi" {
            a.extend(["-vaapi_device".into(), VAAPI_DEVICE.into()]);
        }
        if self.is_gpu() {
            a.extend(["-hwaccel".into(), "auto".into()]);
        }
        a
    }

    /// Fin de la chaîne de filtres de la vidéo encodée (VAAPI : images envoyées à la carte).
    pub fn filter_tail(&self) -> &'static str {
        if self.name == "h264_vaapi" {
            ",format=nv12,hwupload"
        } else {
            ""
        }
    }

    /// Options de l'encodeur (« -c:v … » jusqu'au format des pixels) ; `preset` : celui de libx264.
    pub fn video_args(&self, rate: Rate, preset: &str) -> Vec<String> {
        let s = |x: &str| x.to_string();
        let mut a = vec![s("-c:v"), self.name.clone()];
        let q = |crf: u32| quality_value(&self.name, crf).to_string();
        let bitrate = |a: &mut Vec<String>, kbps: i64, max: i64, buf: bool| {
            a.extend([s("-b:v"), format!("{kbps}k"), s("-maxrate"), format!("{max}k")]);
            if buf {
                a.extend([s("-bufsize"), format!("{}k", 2 * max)]);
            }
        };
        match self.name.as_str() {
            "h264_nvenc" => {
                a.extend([s("-preset"), s(nvenc_preset(preset)), s("-tune"), s("hq"), s("-rc"), s("vbr")]);
                match rate {
                    Rate::Crf(c) => a.extend([s("-cq"), q(c), s("-b:v"), s("0")]),
                    Rate::Bitrate { kbps, max_kbps } => bitrate(&mut a, kbps, max_kbps, true),
                }
                a.extend([s("-pix_fmt"), s("yuv420p")]);
            }
            "h264_qsv" => {
                a.extend([s("-preset"), s(qsv_preset(preset))]);
                match rate {
                    Rate::Crf(c) => a.extend([s("-global_quality"), q(c)]),
                    Rate::Bitrate { kbps, max_kbps } => bitrate(&mut a, kbps, max_kbps, true),
                }
                a.extend([s("-pix_fmt"), s("nv12")]);
            }
            "h264_vaapi" => match rate {
                Rate::Crf(c) => a.extend([s("-rc_mode"), s("CQP"), s("-qp"), q(c)]),
                Rate::Bitrate { kbps, max_kbps } => {
                    a.extend([s("-rc_mode"), s("VBR")]);
                    bitrate(&mut a, kbps, max_kbps, false)
                }
            },
            "h264_amf" => {
                a.extend([s("-quality"), s(if matches!(preset, "medium" | "slow" | "slower" | "veryslow") { "quality" } else { "balanced" })]);
                match rate {
                    Rate::Crf(c) => a.extend([s("-rc"), s("cqp"), s("-qp_i"), q(c), s("-qp_p"), q(c), s("-qp_b"), q(c)]),
                    Rate::Bitrate { kbps, max_kbps } => {
                        a.extend([s("-rc"), s("vbr_peak")]);
                        bitrate(&mut a, kbps, max_kbps, true)
                    }
                }
                a.extend([s("-pix_fmt"), s("nv12")]);
            }
            "h264_videotoolbox" => {
                match rate {
                    Rate::Crf(c) => a.extend([s("-q:v"), q(c)]),
                    Rate::Bitrate { kbps, .. } => a.extend([s("-b:v"), format!("{kbps}k")]),
                }
                a.extend([s("-pix_fmt"), s("yuv420p")]);
            }
            _ => {
                a.extend([s("-preset"), s(preset)]);
                match rate {
                    Rate::Crf(c) => a.extend([s("-crf"), c.to_string()]),
                    Rate::Bitrate { kbps, max_kbps } => bitrate(&mut a, kbps, max_kbps, true),
                }
                a.extend([s("-pix_fmt"), s("yuv420p")]);
            }
        }
        a
    }
}

/// Valeur de qualité de l'encodeur équivalente (à peu près) au crf de libx264. Les encodeurs
/// matériels compressent un peu moins bien à réglage égal : un cran de plus pour NVENC et QSV
/// (fichier de taille voisine, qualité voisine) ; VAAPI et AMF à qp constant : même nombre
/// (fichier plus gros, qualité au moins égale) ; VideoToolbox : 1 à 100, plus haut = meilleur
/// (crf 16 → 70, crf 18 → 64, crf 23 → 49).
pub fn quality_value(encoder: &str, crf: u32) -> u32 {
    match encoder {
        "h264_nvenc" | "h264_qsv" => crf + 1,
        "h264_videotoolbox" => (118 - 3 * crf.min(39) as i64).clamp(1, 100) as u32,
        _ => crf,
    }
}

fn nvenc_preset(x264: &str) -> &'static str {
    match x264 {
        "ultrafast" | "superfast" => "p2",
        "veryfast" | "faster" => "p4",
        "fast" | "medium" => "p5",
        _ => "p6",
    }
}

fn qsv_preset(x264: &str) -> &'static str {
    match x264 {
        "ultrafast" | "superfast" | "veryfast" => "veryfast",
        "faster" => "faster",
        "fast" => "fast",
        "medium" => "medium",
        "slow" => "slow",
        _ => "slower",
    }
}

/// Encodeurs matériels de ce système présents dans la liste de `ffmpeg -encoders`.
pub fn compiled(listing: &str, wanted: &[&'static str]) -> Vec<&'static str> {
    let names: Vec<&str> = listing.lines().filter_map(|l| l.split_whitespace().nth(1)).collect();
    wanted.iter().copied().filter(|w| names.contains(w)).collect()
}

/// Premier encodeur de `wanted` présent dans le ffmpeg et qui marche (`works`), sinon le processeur.
pub fn choose(listing: &str, wanted: &[&'static str], mut works: impl FnMut(&Encoder) -> bool) -> Encoder {
    for name in compiled(listing, wanted) {
        let enc = Encoder { name: name.into() };
        if works(&enc) {
            return enc;
        }
    }
    Encoder::cpu()
}

/// `AM_GPU` : « 0 », « non », « off », « false » coupent la carte graphique.
pub fn disabled_by(value: Option<&str>) -> bool {
    matches!(value.map(|v| v.trim().to_lowercase()).as_deref(), Some("0" | "non" | "off" | "false" | "no"))
}

fn disabled() -> bool {
    disabled_by(std::env::var("AM_GPU").ok().as_deref())
}

/// Essai d'un encodeur : quelques images en qualité constante, puis à débit visé.
fn probe(enc: &Encoder) -> bool {
    [Rate::Crf(20), Rate::Bitrate { kbps: 2000, max_kbps: 3000 }].into_iter().all(|rate| {
        let mut args: Vec<String> = vec!["-hide_banner".into(), "-v".into(), "error".into()];
        if enc.name == "h264_vaapi" {
            args.extend(["-vaapi_device".into(), VAAPI_DEVICE.into()]);
        }
        args.extend(["-f", "lavfi", "-i", "color=c=gray:s=640x360:r=30:d=0.4", "-vf"].map(String::from));
        args.push(format!("format=yuv420p{}", enc.filter_tail()));
        args.extend(enc.video_args(rate, "veryfast"));
        args.extend(["-g", "15", "-an", "-f", "null", "-"].map(String::from));
        run_with_timeout(&args)
    })
}

fn run_with_timeout(args: &[String]) -> bool {
    let Ok(mut child) = Command::new(paths::ffmpeg()).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() < PROBE_TIMEOUT => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Identité du ffmpeg utilisé (le résultat de l'essai en dépend).
fn ffmpeg_key() -> String {
    let p = paths::ffmpeg();
    let (size, mtime) = std::fs::metadata(p).map(|m| (m.len(), util::mtime(p).unwrap_or(0.0) as i64)).unwrap_or((0, 0));
    format!("{} {size} {mtime}", paths::display(p))
}

fn cache_path() -> std::path::PathBuf {
    paths::work().join("gpu.json")
}

fn detect() -> Encoder {
    let listing = crate::procs::run(paths::ffmpeg(), ["-hide_banner", "-encoders"]).1;
    choose(&listing, candidates(), probe)
}

static CHOSEN: Mutex<Option<Encoder>> = Mutex::new(None);

/// Encodeur H.264 à utiliser (carte graphique si elle marche, sinon processeur). Annoncé une fois
/// par programme : « Encodage : carte graphique (h264_nvenc) » ou « Encodage : processeur ».
pub fn h264() -> Encoder {
    let mut chosen = CHOSEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = chosen.as_ref() {
        return e.clone();
    }
    let enc = if disabled() {
        Encoder::cpu()
    } else {
        let key = ffmpeg_key();
        let cached = util::read_json(cache_path()).filter(|c| util::s(c, "ffmpeg") == Some(key.as_str()));
        match cached.as_ref().and_then(|c| util::s(c, "encoder")) {
            Some(name) if name == CPU || candidates().contains(&name) => Encoder { name: name.into() },
            _ => {
                let enc = detect();
                save(&key, &enc, None);
                enc
            }
        }
    };
    progress::say(&format!("Encodage : {}", enc.label()));
    *chosen = Some(enc.clone());
    enc
}

fn save(key: &str, enc: &Encoder, failed: Option<&str>) {
    let mut v = json!({"ffmpeg": key, "encoder": enc.name});
    if let Some(f) = failed {
        v["failed"] = Value::from(f);
    }
    if paths::work().is_dir() {
        let _ = util::write_json(cache_path(), &v, None);
    }
}

/// Encodage par `run` avec l'encodeur retenu ; en cas d'échec sur la carte graphique, refait par
/// le processeur (et la carte n'est plus essayée pour ce ffmpeg).
pub fn encode(mut run: impl FnMut(&Encoder) -> i32) -> i32 {
    let enc = h264();
    let code = run(&enc);
    if code == 0 || !enc.is_gpu() {
        return code;
    }
    give_up(&enc);
    run(&Encoder::cpu())
}

/// La carte graphique a échoué en cours d'encodage : processeur désormais.
pub fn give_up(enc: &Encoder) {
    progress::say(&format!("Encodage par la {} en échec : reprise par le processeur", enc.label()));
    save(&ffmpeg_key(), &Encoder::cpu(), Some(&enc.name));
    *CHOSEN.lock().unwrap_or_else(|e| e.into_inner()) = Some(Encoder::cpu());
}

/// Compilation de whisper.cpp sur cette machine : option CMake du calcul par la carte graphique si
/// sa boîte à outils est installée (`has` : programme trouvé), sinon rien (processeur). macOS :
/// Metal, déjà choisi par whisper.cpp. CUDA (nvcc) avant Vulkan (glslc et vulkaninfo du SDK).
pub fn whisper_cmake_flag(has: impl Fn(&str) -> bool, macos: bool, off: bool) -> Option<(&'static str, &'static str)> {
    if macos || off {
        return None;
    }
    if has("nvcc") {
        return Some(("-DGGML_CUDA=1", "CUDA"));
    }
    if has("glslc") && has("vulkaninfo") {
        return Some(("-DGGML_VULKAN=1", "Vulkan"));
    }
    None
}

/// Option CMake de whisper.cpp pour cette machine (voir `whisper_cmake_flag`).
pub fn whisper_flag() -> Option<(&'static str, &'static str)> {
    whisper_cmake_flag(|n| paths::which(n).is_some(), cfg!(target_os = "macos"), disabled())
}

pub fn main(_args: &[String]) -> i32 {
    if disabled() {
        println!("Carte graphique coupée (AM_GPU) : processeur");
        return 0;
    }
    let listing = crate::procs::run(paths::ffmpeg(), ["-hide_banner", "-encoders"]).1;
    let found = compiled(&listing, candidates());
    println!("ffmpeg : {}", paths::display(paths::ffmpeg()));
    println!("Encodeurs matériels de ce ffmpeg : {}", if found.is_empty() { "aucun".into() } else { found.join(", ") });
    let enc = choose(&listing, candidates(), |e| {
        let ok = probe(e);
        println!("  {} : {}", e.name, if ok { "marche" } else { "ne marche pas ici" });
        ok
    });
    save(&ffmpeg_key(), &enc, None);
    println!("Encodage : {}", enc.label());
    match whisper_flag() {
        Some((_, name)) => println!("Whisper compilé ici : avec {name}"),
        None => println!("Whisper compilé ici : processeur{}", if cfg!(target_os = "macos") { " et Metal" } else { "" }),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "Encoders:\n V..... = Video\n ------\n V....D libx264              libx264 H.264\n V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)\n V..... h264_qsv             H.264 (Intel Quick Sync Video acceleration)\n V....D h264_vaapi           H.264/AVC (VAAPI) (codec h264)\n V....D h264_amf             AMD AMF H.264 Encoder\n";

    #[test]
    fn encoders_found_in_listing() {
        assert_eq!(compiled(LISTING, &["h264_nvenc", "h264_vaapi", "h264_qsv"]), vec!["h264_nvenc", "h264_vaapi", "h264_qsv"]);
        assert_eq!(compiled(LISTING, &["h264_videotoolbox"]), Vec::<&str>::new());
        // ffmpeg livré sur Linux : aucun encodeur matériel utilisable.
        assert!(compiled(" V....D libx264  libx264\n V..... h264_v4l2m2m  V4L2\n", &["h264_nvenc", "h264_vaapi", "h264_qsv"]).is_empty());
    }

    #[test]
    fn first_working_encoder_else_cpu() {
        let wanted = ["h264_nvenc", "h264_vaapi", "h264_qsv"];
        let mut tried = vec![];
        let enc = choose(LISTING, &wanted, |e| {
            tried.push(e.name.clone());
            e.name == "h264_qsv"
        });
        assert_eq!(enc.name, "h264_qsv");
        assert_eq!(tried, ["h264_nvenc", "h264_vaapi", "h264_qsv"]);
        assert_eq!(choose(LISTING, &wanted, |_| false), Encoder::cpu());
        assert!(!Encoder::cpu().is_gpu());
        assert_eq!(Encoder::cpu().label(), "processeur");
        assert_eq!(Encoder { name: "h264_nvenc".into() }.label(), "carte graphique (h264_nvenc)");
    }

    #[test]
    fn switch_off() {
        assert!(disabled_by(Some("0")));
        assert!(disabled_by(Some("non")));
        assert!(disabled_by(Some("OFF")));
        assert!(!disabled_by(Some("1")));
        assert!(!disabled_by(None));
    }

    #[test]
    fn encoder_arguments() {
        let cpu = Encoder::cpu();
        assert_eq!(cpu.video_args(Rate::Crf(16), "veryfast").join(" "), "-c:v libx264 -preset veryfast -crf 16 -pix_fmt yuv420p");
        assert_eq!(
            cpu.video_args(Rate::Bitrate { kbps: 8000, max_kbps: 12000 }, "fast").join(" "),
            "-c:v libx264 -preset fast -b:v 8000k -maxrate 12000k -bufsize 24000k -pix_fmt yuv420p"
        );
        assert!(cpu.input_args().is_empty());
        assert_eq!(cpu.filter_tail(), "");
        let nv = Encoder { name: "h264_nvenc".into() };
        assert_eq!(nv.video_args(Rate::Crf(16), "veryfast").join(" "), "-c:v h264_nvenc -preset p4 -tune hq -rc vbr -cq 17 -b:v 0 -pix_fmt yuv420p");
        assert_eq!(nv.input_args(), ["-hwaccel", "auto"]);
        let va = Encoder { name: "h264_vaapi".into() };
        assert_eq!(va.input_args(), ["-vaapi_device", VAAPI_DEVICE, "-hwaccel", "auto"]);
        assert_eq!(va.filter_tail(), ",format=nv12,hwupload");
        assert_eq!(va.video_args(Rate::Crf(18), "veryfast").join(" "), "-c:v h264_vaapi -rc_mode CQP -qp 18");
        let qsv = Encoder { name: "h264_qsv".into() };
        assert_eq!(qsv.video_args(Rate::Crf(18), "veryfast").join(" "), "-c:v h264_qsv -preset veryfast -global_quality 19 -pix_fmt nv12");
        let amf = Encoder { name: "h264_amf".into() };
        assert!(amf.video_args(Rate::Crf(18), "veryfast").join(" ").contains("-rc cqp -qp_i 18 -qp_p 18"));
        let vt = Encoder { name: "h264_videotoolbox".into() };
        assert_eq!(vt.video_args(Rate::Crf(16), "veryfast").join(" "), "-c:v h264_videotoolbox -q:v 70 -pix_fmt yuv420p");
        assert_eq!(vt.video_args(Rate::Bitrate { kbps: 8000, max_kbps: 12000 }, "fast").join(" "), "-c:v h264_videotoolbox -b:v 8000k -pix_fmt yuv420p");
        assert_eq!(quality_value("h264_videotoolbox", 18), 64);
        assert_eq!(quality_value("h264_videotoolbox", 23), 49);
    }

    #[test]
    fn whisper_gpu_build() {
        let has = |list: &'static [&'static str]| move |n: &str| list.contains(&n);
        assert_eq!(whisper_cmake_flag(has(&["nvcc", "glslc", "vulkaninfo"]), false, false), Some(("-DGGML_CUDA=1", "CUDA")));
        assert_eq!(whisper_cmake_flag(has(&["glslc", "vulkaninfo"]), false, false), Some(("-DGGML_VULKAN=1", "Vulkan")));
        assert_eq!(whisper_cmake_flag(has(&["vulkaninfo"]), false, false), None);
        assert_eq!(whisper_cmake_flag(has(&["nvcc"]), true, false), None);
        assert_eq!(whisper_cmake_flag(has(&["nvcc"]), false, true), None);
    }
}
