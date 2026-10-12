//! Prépare la page « Téléchargements » : la vidéo entière en 720p, 1080p et 4K.
//!
//! Sources (celles qui existent) :
//!   720p   work/moments/montage.mp4   (copie légère de la page des moments, make_moments)
//!   1080p  out/montage_apercu.mp4     (auto-montage render apercu)
//!   4K     out/montage_4k.mp4         (auto-montage render final)
//! Un fichier de moins de 15 Mo est publié tel quel ; au-delà, il est découpé en morceaux de
//! 14 Mo que la page rassemble en un seul MP4 au téléchargement (limite de 15 Mo par fichier
//! publié). Une page tient au plus 256 Mo : si l'ensemble dépasse BUDGET, la copie 4K de la
//! page est réencodée (2 passes) juste assez pour tenir ; le rendu 4K d'origine n'est pas touché.
//!
//! Sorties : work/downloads/index.html   page à publier (gabarit selection/downloads.html)
//!           work/downloads/files/*      fichiers et morceaux, noms datés du rendu
//!           work/downloads/publish.json lots de fichiers à publier (64 Mo max par publication)
//!                                       et fichiers de l'ancienne version à retirer (null)

use crate::make_moments::{devnull, html_escape, iso, py_mtime, utc_of};
use crate::moments_lib::{edit_size, res_label};
use crate::paths;
use crate::procs;
use crate::progress::duration_of;
use crate::pyjson;
use crate::util::{self, num, read_json, round_int, round_to};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MB: u64 = 1024 * 1024;
const SINGLE_MAX: u64 = 15 * MB - 64 * 1024; // un fichier publié : 15 Mo max
const PART: u64 = 14 * MB;
const BUDGET: u64 = 245 * MB; // page entière (256 Mo max), marge pour la page elle-même
const BATCH: u64 = 60 * MB; // une publication : 64 Mo max

fn duration(path: &Path) -> f64 {
    duration_of(&procs::run(paths::ffmpeg(), [std::ffi::OsStr::new("-i"), path.as_os_str()]).2)
}

fn p(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Copie réencodée (2 passes) de src qui pèse environ target_bytes, en cache.
fn fit(out: &Path, src: &Path, target_bytes: u64) -> Result<PathBuf, String> {
    let base = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dst = out.join(format!("fit_{base}"));
    let key = format!("{}:{}", pyjson::float_repr(py_mtime(src).unwrap_or(0.0)), target_bytes);
    let key_path = out.join(format!("fit_{base}.key"));
    if dst.exists() && fs::read_to_string(&key_path).map(|k| k == key).unwrap_or(false) {
        return Ok(dst);
    }
    let audio: i64 = 192_000;
    let video = (target_bytes as f64 * 8.0 * 0.97 / duration(src)) as i64 - audio;
    let log = p(&out.join("fit_pass"));
    let common: Vec<String> = [
        "-v", "error", "-y", "-i", &p(src), "-c:v", "libx264", "-preset", "medium", "-b:v", &video.to_string(), "-pix_fmt", "yuv420p",
        "-passlogfile", &log,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    println!("Réencodage de {base} à {:.1} Mb/s pour tenir sur la page…", video as f64 / 1e6);
    let mut pass1 = common.clone();
    pass1.extend(["-pass", "1", "-an", "-f", "mp4", devnull()].iter().map(|s| s.to_string()));
    let code = procs::call(paths::ffmpeg(), &pass1);
    if code != 0 {
        return Err(format!("ffmpeg (réencodage, 1re passe) : code {code}"));
    }
    let mut pass2 = common;
    pass2.extend(["-pass", "2", "-c:a", "aac", "-b:a", &audio.to_string(), "-movflags", "+faststart", &p(&dst)].iter().map(|s| s.to_string()));
    let code = procs::call(paths::ffmpeg(), &pass2);
    if code != 0 {
        return Err(format!("ffmpeg (réencodage, 2e passe) : code {code}"));
    }
    for f in util::list_dir(out) {
        if f.starts_with("fit_pass") {
            let _ = fs::remove_file(out.join(f));
        }
    }
    fs::write(&key_path, key).map_err(|e| e.to_string())?;
    Ok(dst)
}

pub fn main(_args: &[String]) -> i32 {
    match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Erreur : {e}");
            1
        }
    }
}

struct Source {
    id: &'static str,
    label: &'static str,
    res: String,
    path: PathBuf,
    note: &'static str,
}

fn run() -> Result<(), String> {
    let root = paths::root();
    let work = paths::work();
    let out = work.join("downloads");
    let files = out.join("files");

    let rush_txt = util::read_text(work.join("rush.txt")).ok_or("work/rush.txt illisible")?;
    let rush = Path::new(&rush_txt).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // Format du rush (9:16, 16:9, 4:3…).
    let size = edit_size(read_json(paths::data().join("edit.json")).as_ref());
    // « 720x1280 » en 9:16 : petit côté de w, au format du rush
    let res = |w: i64| res_label(size.0, size.1, w);

    let sources = vec![
        Source {
            id: "720p",
            label: "Légère",
            res: res(720),
            path: work.join("moments").join("montage.mp4"),
            note: "Copie compressée de la vidéo entière, pour la partager ou la regarder sur téléphone.",
        },
        Source {
            id: "1080p",
            label: "1080p",
            res: res(1080),
            path: paths::out().join("montage_apercu.mp4"),
            note: "Rendu de l'aperçu, à la résolution de la composition.",
        },
        Source { id: "4k", label: "4K", res: res(2160), path: paths::out().join("montage_4k.mp4"), note: "Rendu final (./scripts/render.sh final)." },
    ];

    util::mkdirs(&files);
    let available: Vec<&Source> = sources.iter().filter(|s| s.path.exists()).collect();
    let sizes: Vec<u64> = available.iter().map(|s| fs::metadata(&s.path).map(|m| m.len()).unwrap_or(0)).collect();
    let total: u64 = sizes.iter().sum();
    // Trop lourd pour une page : la plus grosse copie (la 4K) est réencodée pour tenir.
    let mut fitted: Option<(usize, PathBuf)> = None;
    if total > BUDGET {
        // max() de Python : la première des plus grosses.
        let mut big = 0;
        for (i, s) in sizes.iter().enumerate() {
            if *s > sizes[big] {
                big = i;
            }
        }
        let target = BUDGET - (total - sizes[big]);
        fitted = Some((big, fit(&out, &available[big].path, target)?));
    }

    let publish_path = out.join("publish.json");
    let mut previous: BTreeSet<String> = BTreeSet::new();
    if publish_path.exists() {
        let data = read_json(&publish_path).ok_or("work/downloads/publish.json illisible")?;
        previous = util::arr(&data, "published").iter().filter_map(|v| v.as_str().map(String::from)).collect();
    }

    let apercu = paths::out().join("montage_apercu.mp4");
    let mut versions: Vec<Value> = vec![];
    let mut current: Vec<String> = vec![];
    for f in util::list_dir(&files) {
        let _ = fs::remove_file(files.join(f));
    }
    for (i, s) in available.iter().enumerate() {
        let rendered = utc_of(py_mtime(&s.path).unwrap_or(0.0));
        let tag = rendered.format("%Y%m%d-%H%M%S").to_string();
        let src = match &fitted {
            Some((k, path)) if *k == i => path.clone(),
            _ => s.path.clone(),
        };
        let size = fs::metadata(&src).map(|m| m.len()).map_err(|e| e.to_string())?;
        let base = format!("{rush}_{}_{tag}", s.id);
        let mut names = vec![];
        if size <= SINGLE_MAX {
            let name = format!("files/{base}.mp4");
            fs::copy(&src, files.join(format!("{base}.mp4"))).map_err(|e| e.to_string())?;
            names.push(name);
        } else {
            // Morceaux bruts (en .mp4 : type servi par les pages), rassemblés dans l'ordre par la page.
            let mut input = fs::File::open(&src).map_err(|e| e.to_string())?;
            let mut k = 0;
            while k * PART < size {
                let file = format!("{base}.part{:02}.mp4", k + 1);
                let mut buf = Vec::with_capacity(PART as usize);
                (&mut input).take(PART).read_to_end(&mut buf).map_err(|e| e.to_string())?;
                fs::File::create(files.join(&file)).and_then(|mut f| f.write_all(&buf)).map_err(|e| e.to_string())?;
                names.push(format!("files/{file}"));
                k += 1;
            }
        }
        current.extend(names.iter().cloned());
        let stale = s.id != "720p"
            && apercu.exists()
            && py_mtime(&s.path).unwrap_or(0.0) < py_mtime(&apercu).unwrap_or(0.0) - 60.0;
        versions.push(json!({
            "id": s.id, "label": s.label, "res": s.res, "note": s.note,
            "filename": format!("{rush}_{}_{}.mp4", s.id, rendered.format("%Y-%m-%d_%Hh%M")),
            "size": size, "duration": num(round_to(duration(&src), 1)), "renderedAt": iso(&rendered),
            "parts": names,
            "reencoded": match &fitted { Some((k, _)) if *k == i => json!(round_int(sizes[i] as f64 / MB as f64)), _ => Value::Null },
            // Rendu plus ancien que la version de travail actuelle (ex. 4K d'un montage précédent).
            "stale": stale,
        }));
    }

    let moments_url = util::read_text(work.join("moments_url.txt")).map(Value::String).unwrap_or(Value::Null);
    let page = fs::read_to_string(root.join("selection").join("downloads.html")).map_err(|e| e.to_string())?;
    let page = page.replace("__RUSH__", &html_escape(&rush));
    let data = json!({"rush": rush, "versions": versions, "momentsPage": moments_url});
    let page = page.replace("/*__DATA__*/null", &pyjson::dumps_utf8(&data).replace('<', "\\u003c"));
    fs::write(out.join("index.html"), page).map_err(|e| e.to_string())?;

    // Lots de publication : 64 Mo max par publication, les anciens fichiers à retirer.
    let mut batches: Vec<Vec<String>> = vec![];
    let (mut cur, mut cur_size): (Vec<String>, u64) = (vec![], 0);
    for name in &current {
        let s = fs::metadata(out.join(name)).map(|m| m.len()).unwrap_or(0);
        if !cur.is_empty() && cur_size + s > BATCH {
            batches.push(std::mem::take(&mut cur));
            cur_size = 0;
        }
        cur.push(name.clone());
        cur_size += s;
    }
    if !cur.is_empty() {
        batches.push(cur);
    }
    let cur_set: BTreeSet<&String> = current.iter().collect();
    let remove: Vec<&String> = previous.iter().filter(|n| !cur_set.contains(n)).collect();
    util::write_json(&publish_path, &json!({"batches": batches, "remove": remove, "published": current}), Some(1))
        .map_err(|e| e.to_string())?;
    for v in &versions {
        let extra = match v["reencoded"].as_i64() {
            Some(r) if r != 0 => format!(", réencodée depuis {r} Mo"),
            _ => String::new(),
        };
        println!(
            "{:>5} : {:6.1} Mo en {} fichier(s){extra}{}",
            v["id"].as_str().unwrap_or(""),
            v["size"].as_f64().unwrap_or(0.0) / MB as f64,
            util::arr(v, "parts").len(),
            if v["stale"] == json!(true) { " (ancienne)" } else { "" }
        );
    }
    println!("{} publication(s), {} ancien(s) fichier(s) à retirer -> work/downloads/", batches.len(), remove.len());
    Ok(())
}
