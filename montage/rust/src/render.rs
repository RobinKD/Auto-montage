//! Rendu du montage avec HyperFrames (render_hyperframes), en deux temps :
//!   auto-montage render apercu   1080p (format du rush), rapide, pour valider le montage
//!   auto-montage render final    4K une fois le montage validé
//! Le mode final encode d'abord la source 4K des seules images gardées (make_hd), puis rend la
//! même page à l'échelle 2 : texte net et image 4K native.
//! Puis le son de la vidéo rendue est amené au volume voulu (loudness.rs, work/son.json « loudness »).
//! Contrôle à la fin : dimensions, durée et saturation du son de la vidéo rendue.

use crate::paths;
use crate::procs;
use crate::progress::say;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Instant;

pub fn main(args: &[String]) -> i32 {
    let mode = args.first().map(String::as_str).unwrap_or("");
    let out = match mode {
        "apercu" => "out/montage_apercu.mp4",
        "final" => "out/montage_4k.mp4",
        _ => {
            eprintln!("Usage : auto-montage render apercu|final");
            return 1;
        }
    };
    if mode == "final" {
        let code = sub(&["make_hd"]);
        if code != 0 {
            return code;
        }
    }
    crate::util::mkdirs(paths::out());
    // Rendu sous un nom provisoire, renommé à la fin : un rendu interrompu (« Annuler ») laisse la
    // version précédente intacte.
    let part = format!("{}.part.mp4", out.strip_suffix(".mp4").unwrap_or(out));
    // Empreinte des réglages du montage rendu (build_edit), lue avant le rendu : gardée pour la vidéo.
    let print = crate::util::read_json(paths::data().join("edit.json")).and_then(|e| e.get("settingsPrint").cloned());
    let start = Instant::now();
    let code = sub(&["render_hyperframes", mode, &part]);
    if code != 0 {
        return code;
    }
    let (part_path, out_path) = (rel(&part), rel(out));
    let _ = std::fs::remove_file(&out_path); // mv -f (Windows : rename ne remplace pas toujours)
    if let Err(e) = std::fs::rename(&part_path, &out_path) {
        eprintln!("mv {part} {out} : {e}");
        return 1;
    }
    say(&format!("Rendu en {} s", start.elapsed().as_secs()));
    keep_print(out, print);
    // Volume normalisé (son.json « loudness ») : son de la vidéo rendue mesuré puis corrigé.
    if let Err(e) = crate::loudness::after_render(&out_path) {
        say(&format!("Attention : volume non normalisé ({e})"));
    }
    // Sous-titres en fichiers (out/montage.srt, .vtt), à côté de la vidéo.
    match crate::subtitles::write_all() {
        Ok(f) if !f.is_empty() => say("Sous-titres : out/montage.srt, out/montage.vtt"),
        Ok(_) => {}
        Err(e) => say(&format!("Attention : sous-titres non exportés ({e})")),
    }
    check(out)
}

/// Note l'empreinte des réglages de la vidéo rendue (work/video_reglages.json, par fichier de out/) :
/// l'interface dit quels réglages ont changé depuis (moments_lib::settings_print).
fn keep_print(out: &str, print: Option<serde_json::Value>) {
    let path = paths::work().join(crate::moments_lib::VIDEO_PRINTS);
    let mut all = match crate::util::read_json(&path) {
        Some(serde_json::Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    let name = out.rsplit('/').next().unwrap_or(out).to_string();
    match print {
        Some(p) => all.insert(name, p),
        None => all.remove(&name), // montage sans empreinte (version précédente) : comparé à sa date
    };
    let _ = crate::util::write_json(&path, &serde_json::Value::Object(all), Some(1));
}

/// Chemin du projet écrit avec des « / » (out/montage_apercu.mp4), sur le disque.
fn rel(p: &str) -> std::path::PathBuf {
    p.split('/').fold(paths::root().to_path_buf(), |acc, x| acc.join(x))
}

/// Sous-commande de ce programme, lancée depuis la racine du projet ; renvoie son code.
fn sub(args: &[&str]) -> i32 {
    match Command::new(paths::self_exe()).args(args).current_dir(paths::root()).status() {
        Ok(s) => {
            let c = procs::py_code(&s);
            if c < 0 {
                128 - c
            } else {
                c
            }
        }
        Err(e) => {
            eprintln!("auto-montage {} ne se lance pas : {e}", args[0]);
            127
        }
    }
}

/// Taille de l'image dans la sortie de « ffmpeg -i » (premier « 1080x1920 » d'une ligne Video:).
fn video_size(info: &str) -> String {
    for line in info.lines().filter(|l| l.contains("Video:")) {
        for w in line.split(", ") {
            let first = w.split(' ').next().unwrap_or("");
            let digits = first.replace('x', "");
            if w.contains('x') && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                return w.split(' ').next().unwrap_or("?").to_string();
            }
        }
    }
    "?".into()
}

/// Échantillons saturés tolérés : quelques-uns sont inaudibles (une crête isolée que l'encodage AAC
/// a fait dépasser), au plus 10 ou 1 sur 100 000 (moins de 0,5 par seconde de son).
fn clip_tolerance(samples: u64) -> u64 {
    (samples / 100_000).max(10)
}

/// Contrôle : dimensions, durée et saturation du son (code 1 si plus d'échantillons saturés que
/// `clip_tolerance`).
fn check(out: &str) -> i32 {
    let path = rel(out);
    let ff = paths::ffmpeg();
    let size = video_size(&procs::ffmpeg_info(&path));
    let mut child = match Command::new(ff)
        .args(["-v", "error", "-i"])
        .arg(&path)
        .args(["-ac", "1", "-ar", "48000", "-f", "s16le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ffmpeg ne se lance pas : {e}");
            return 1;
        }
    };
    let mut stdout = child.stdout.take().unwrap();
    let (mut samples, mut clipped) = (0u64, 0u64);
    let mut buf = vec![0u8; 1 << 16];
    let mut carry: Option<u8> = None;
    loop {
        let n = match stdout.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut bytes: &[u8] = &buf[..n];
        let mut first = [0u8; 2];
        if let Some(c) = carry.take() {
            first[0] = c;
            first[1] = bytes[0];
            count(&first, &mut samples, &mut clipped);
            bytes = &bytes[1..];
        }
        let even = bytes.len() / 2 * 2;
        count(&bytes[..even], &mut samples, &mut clipped);
        if even < bytes.len() {
            carry = Some(bytes[even]);
        }
    }
    let _ = child.wait();
    let secs = samples as f64 / 48000.0;
    if clipped == 0 {
        say(&format!("{out} : {size}, {secs:.1} s, aucun échantillon saturé"));
        0
    } else if clipped <= clip_tolerance(samples) {
        say(&format!("{out} : {size}, {secs:.1} s, {clipped} échantillons saturés (inaudible, toléré)"));
        0
    } else {
        say(&format!("{out} : {size}, {secs:.1} s, {clipped} échantillons saturés (au-delà des {} tolérés)", clip_tolerance(samples)));
        1
    }
}

fn count(bytes: &[u8], samples: &mut u64, clipped: &mut u64) {
    for s in bytes.chunks_exact(2) {
        let v = i16::from_le_bytes([s[0], s[1]]) as f64 / 32768.0;
        *samples += 1;
        if v.abs() > 0.98 {
            *clipped += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_from_info() {
        let info = "  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1080x1920 [SAR 1:1 DAR 9:16], 2105 kb/s, 30 fps";
        assert_eq!(video_size(info), "1080x1920");
        assert_eq!(video_size("rien"), "?");
    }

    #[test]
    fn a_few_clipped_samples_are_tolerated() {
        assert_eq!(clip_tolerance(48000), 10); // 1 s de son
        assert_eq!(clip_tolerance(976 * 48000), 468); // 16 min 16 s
    }
}
