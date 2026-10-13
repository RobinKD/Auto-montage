//! Données servies aux pages : style des sous-titres, effets proposés, son de la voix,
//! habillage, éléments de style, page des moments, téléchargements, rushes et montages.

use super::common::*;
use super::jobs::{self, load_rates, rate, RATES};
use crate::moments_lib::{self, edit_size, hf_preview_files, hidden_effects, res_label, sound_catalog, wav_len, DEFAULTS_VERSION, SOUNDS as DEFAULT_SOUNDS, VISUALS, VOICES};
use crate::{fonts_lib, memoire, paths, procs, pyjson, util};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

// --- Style des sous-titres (page des moments) et effets proposés (page des rushes) -----
// Polices : fonts_lib. Les fichiers sont servis à la page par leur identifiant
// (/api/fonts/<id>), seulement s'ils font partie de la liste des polices utilisables.
pub fn effects_path() -> PathBuf {
    work().join("effets_page.json")
}

/// Effets fournis par défaut : mêmes identifiants que la page des moments (sfxCatalog, VFX_CATALOG).
pub fn public_effects() -> Vec<Value> {
    let mut v: Vec<Value> = DEFAULT_SOUNDS.iter().map(|(i, label, _, _)| json!({"id": i, "label": label, "kind": "son"})).collect();
    v.push(json!({"id": "voix", "label": "Voix modifiée", "kind": "voix"}));
    v.push(json!({"id": "gainvoix", "label": "Volume de la voix par moment", "kind": "voix"}));
    v.extend(VISUALS.iter().map(|(i, label)| json!({"id": i, "label": label, "kind": "visuel"})));
    v
}

/// Polices utilisables, et fichier de chaque identifiant servi par /api/fonts/.
pub fn font_files() -> (Vec<fonts_lib::Family>, Vec<(String, PathBuf)>) {
    let fonts = fonts_lib::list_fonts();
    let files = fonts.iter().flat_map(|f| f.faces.values().map(|p| (fonts_lib::file_id(p), p.clone()))).collect();
    (fonts, files)
}

fn range(r: (i64, i64)) -> Value {
    json!([r.0, r.1])
}

pub fn caption_style_info() -> Value {
    let (fonts, _) = font_files();
    let mut style = fonts_lib::read_style();
    let family = fonts_lib::resolve(&style, Some(&fonts));
    style.insert("font".into(), family.map(|f| json!(f.id)).unwrap_or(Value::Null));
    let list: Vec<Value> = fonts
        .iter()
        .map(|f| {
            let faces: Map<String, Value> =
                f.faces.iter().map(|(w, p)| (w.to_string(), json!(format!("/api/fonts/{}", fonts_lib::file_id(p))))).collect();
            json!({"id": f.id, "family": f.family, "source": f.source, "faces": faces})
        })
        .collect();
    json!({"style": style, "fonts": list,
           "sizeRange": range(fonts_lib::SIZE_RANGE), "shadowRange": range(fonts_lib::SHADOW_RANGE),
           "blurRange": range(fonts_lib::BLUR_RANGE), "offsetMax": fonts_lib::OFFSET_MAX as i64,
           "linesRange": range(fonts_lib::LINES_RANGE), "lineSizeRange": range(fonts_lib::LINE_SIZE_RANGE),
           "default": fonts_lib::default_style()})
}

/// Style des sous-titres pour l'aperçu de la page (police servie par /api/fonts/).
pub fn caption_style_for_page() -> Value {
    let style = fonts_lib::read_style();
    let Some(family) = fonts_lib::resolve(&style, None) else { return Value::Null };
    let weight = style.get("weight").and_then(Value::as_i64).unwrap_or(700);
    let faces = fonts_lib::faces_for(&family, weight);
    let g = |k: &str| style.get(k).cloned().unwrap_or(Value::Null);
    json!({"family": "AM Sous-titres", "label": family.family,
           "files": faces.iter().map(|(w, p)| json!({"url": format!("/api/fonts/{}", fonts_lib::file_id(p)), "weight": w.to_string()})).collect::<Vec<_>>(),
           "weight": g("weight"), "size": g("size"), "uppercase": g("uppercase"), "color": g("color"),
           "shadow": g("shadow"), "blur": g("blur"), "ox": g("ox"), "oy": g("oy"),
           "lines": g("lines"), "lineSizes": g("lineSizes")})
}

// --- Éléments de style (page « Rushes et montages ») ---------------------------------
// Sons personnels : bibliothèque commune à tous les montages, convertis en WAV dans
// public/sfx/perso/ ; ils rejoignent les effets de la page des moments (« perso-<nom> ») et
// Claude peut les placer (edit_choices.json, « sfx »). Vidéos d'exemple : work/style/videos/,
// avec une planche de 12 images (<nom>.planche.jpg) que Claude regarde pour s'en inspirer.
pub fn sounds_dir() -> PathBuf {
    paths::public().join("sfx").join("perso")
}
pub fn style_videos_dir() -> PathBuf {
    work().join("style").join("videos")
}
pub fn style_analyse_dir() -> PathBuf {
    work().join("style").join("analyse")
}
pub const AUDIO_EXT: [&str; 12] = [".wav", ".mp3", ".m4a", ".aac", ".ogg", ".oga", ".opus", ".flac", ".aif", ".aiff", ".webm", ".mp4"];

pub fn sound_id(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    static NON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());
    let stem = splitext(basename(name)).0;
    let ascii: String = stem.nfkd().filter(|c| c.is_ascii()).collect::<String>().to_lowercase();
    let slug = NON.replace_all(&ascii, "-").trim_matches('-').to_string();
    let slug: String = slug.chars().take(40).collect();
    if slug.is_empty() {
        "son".into()
    } else {
        slug
    }
}

pub fn style_videos() -> Vec<String> {
    util::list_dir(style_videos_dir()).into_iter().filter(|n| is_video(n) && !n.starts_with('.')).collect()
}

pub fn style_seconds() -> f64 {
    style_videos().iter().map(|n| media_seconds(&style_videos_dir().join(n))).sum()
}

pub fn style_info() -> Value {
    let mut sons = vec![];
    for n in util::list_dir(sounds_dir()) {
        if let Some(stem) = n.strip_suffix(".wav") {
            let size = fs::metadata(sounds_dir().join(&n)).map(|m| m.len()).unwrap_or(0);
            sons.push(json!({"id": format!("perso-{stem}"), "label": stem.replace('-', " "), "size": size}));
        }
    }
    let mut videos = vec![];
    for n in util::list_dir(style_videos_dir()) {
        if is_video(&n) && !n.starts_with('.') {
            let sheet = style_videos_dir().join(format!("{}.planche.jpg", splitext(&n).0));
            let size = fs::metadata(style_videos_dir().join(&n)).map(|m| m.len()).unwrap_or(0);
            videos.push(json!({"name": n, "size": size, "sheet": sheet.exists()}));
        }
    }
    json!({"sons": sons, "videos": videos})
}

/// Planche de 12 images réparties sur la vidéo (4 x 3, 360 px de large chacune).
pub fn make_contact_sheet(video: &Path) {
    let mut duration = media_seconds(video);
    if duration == 0.0 {
        duration = 12.0;
    }
    let v = video.to_string_lossy();
    let out = format!("{}.planche.jpg", splitext(&v).0);
    let vf = format!("fps={:.6},scale=360:-2,tile=4x3", 12.0 / duration);
    procs::run(paths::ffmpeg(), ["-v", "error", "-y", "-i", &v, "-vf", &vf, "-frames:v", "1", "-q:v", "4", &out]);
}

/// Analyse des vidéos d'exemple : mesures, résultat de Claude, durée prévue d'une analyse.
pub fn style_analysis() -> Value {
    let load = |name: &str| util::read_json(style_analyse_dir().join(name));
    let (facts, result) = (load("faits.json"), load("resultat.json"));
    let videos = style_videos();
    let rates = load_rates();
    let seconds = style_seconds();
    let get = |k: &str, d: f64| rates.get(k).and_then(Value::as_f64).unwrap_or(d);
    let estimate = get("Analyse des vidéos d'exemple", 0.6) * seconds + get("étape:mesures des vidéos d'exemple", 3.0)
        + get("étape:analyse par Claude", 150.0);
    let analysed: Vec<Value> = facts
        .as_ref()
        .and_then(|f| f.get("videos"))
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|v| v.get("video").cloned().unwrap_or(Value::Null)).collect())
        .unwrap_or_default();
    let stale = util::truthy(facts.as_ref()) && analysed != videos.iter().map(|v| json!(v)).collect::<Vec<_>>();
    let at = if util::truthy(result.as_ref()) {
        util::mtime(style_analyse_dir().join("resultat.json")).map(|t| json!(iso_local_secs(t))).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    json!({"videos": videos, "seconds": util::num(util::round_to(seconds, 1)), "estimate": util::round_int(estimate),
           "facts": facts, "result": result, "stale": stale, "at": at})
}

/// Effets écartés (« hidden ») ou à décocher de tous les moments au prochain passage sur la
/// page des moments (« purge », demandé depuis la page « Rushes et montages »).
pub fn effects_file(key: &str) -> Vec<String> {
    match util::read_json(effects_path()) {
        Some(Value::Object(m)) => m
            .get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|h| h.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        _ => vec![],
    }
}

/// Volume de tous les effets sonores (1 = 100 %).
pub fn effects_volume() -> f64 {
    match util::read_json(effects_path()) {
        Some(Value::Object(m)) => match m.get("volume") {
            None => 1.0,
            Some(v) => py_float(v).unwrap_or(1.0),
        },
        _ => 1.0,
    }
}

/// Sons par défaut écartés tant qu'ils ne sont pas cochés.
pub fn effects_hidden() -> Vec<String> {
    hidden_effects(&effects_path())
}

pub fn effects_info() -> Value {
    let hidden = effects_hidden();
    let mut items = public_effects();
    for s in util::arr(&style_info(), "sons") {
        items.push(json!({"id": s["id"], "label": capitalize(s["label"].as_str().unwrap_or("")), "kind": "perso"}));
    }
    // Écoute sur la page « Rushes et montages » : fichier(s) de chaque son.
    let mut files: Map<String, Value> = Map::new();
    for (i, _, f, _) in DEFAULT_SOUNDS.iter() {
        if let Some(f) = f {
            files.insert(i.to_string(), json!([format!("/sfx-defaut/{f}")]));
        }
    }
    files.insert("clavier".into(), json!([1, 2, 3, 4, 1, 2].iter().map(|k| format!("/sfx-defaut/key-{k}.wav")).collect::<Vec<_>>()));
    for e in &items {
        if e["kind"] == "perso" {
            let id = e["id"].as_str().unwrap_or("");
            files.insert(id.to_string(), json!([format!("/sfx/perso/{}.wav", id.get(6..).unwrap_or(""))]));
        }
    }
    let effects: Vec<Value> = items
        .into_iter()
        .map(|mut e| {
            let id = e["id"].as_str().unwrap_or("").to_string();
            if let Some(f) = files.get(&id) {
                e["audio"] = f.clone();
            }
            e["shown"] = json!(!hidden.contains(&id));
            e
        })
        .collect();
    json!({"effects": effects, "purge": effects_file("purge"), "volume": util::num(effects_volume())})
}

pub fn save_effects(body: &Map<String, Value>) -> Value {
    let ok = |items: Option<&Value>| -> Vec<String> {
        let mut v: Vec<String> = match items {
            Some(Value::Array(a)) => a.iter().filter_map(|h| h.as_str()).filter(|h| safe_id(h)).map(String::from).collect(),
            _ => vec![],
        };
        v.sort();
        v.dedup();
        v
    };
    // Seules les clés envoyées changent (effets écartés, à décocher, volume général).
    let hidden = if body.contains_key("hidden") { ok(body.get("hidden")) } else { effects_hidden() };
    let purge = if body.contains_key("purge") { ok(body.get("purge")) } else { effects_file("purge") };
    let volume = match body.get("volume") {
        Some(v) => py_float(v).map(|x| x.clamp(0.0, 2.0)).unwrap_or_else(effects_volume),
        None => effects_volume(),
    };
    let _ = write_json(
        effects_path(),
        &json!({"hidden": hidden, "purge": purge, "volume": util::num(util::round_to(volume, 2)), "defauts": DEFAULTS_VERSION}),
        Some(1),
        true,
    );
    effects_info()
}

// Son de la voix (page des moments) : réglages du montage en cours dans work/son.json
// (volume, nettoyage, bruits à atténuer), analyse par analyze_sound.
pub fn sound_path() -> PathBuf {
    work().join("son.json")
}

pub fn sound_settings() -> Map<String, Value> {
    match util::read_json(sound_path()) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

pub fn save_sound(body: &Map<String, Value>) -> Option<Map<String, Value>> {
    let mut s = sound_settings();
    if let Some(v) = body.get("voice") {
        s.insert("voice".into(), util::num(util::round_to(py_float(v)?.clamp(0.0, 2.0), 2)));
    }
    if let Some(v) = body.get("denoise") {
        s.insert("denoise".into(), json!(py_int(v)?.clamp(0, 3)));
    }
    for key in ["highpass", "declick", "clarity"] {
        if let Some(v) = body.get(key) {
            s.insert(key.into(), json!(util::truthy(Some(v))));
        }
    }
    if let Some(v) = body.get("mute") {
        let Value::Array(items) = v else { return None };
        let mut mute = vec![];
        for item in items {
            let Value::Array(pair) = item else { return None };
            if pair.len() != 2 {
                return None;
            }
            let (a, b) = (py_float(&pair[0])?, py_float(&pair[1])?);
            if b > a {
                mute.push(json!([util::num(util::round_to(a, 2)), util::num(util::round_to(b, 2))]));
            }
        }
        mute.truncate(200);
        s.insert("mute".into(), Value::Array(mute));
    }
    util::mkdirs(work());
    let _ = write_json(sound_path(), &Value::Object(s.clone()), Some(1), true);
    Some(s)
}

pub fn habillage_path() -> PathBuf {
    work().join("habillage.json") // zooms automatiques, animation des sous-titres, cadrage sur qui parle
}
pub fn tutorial_seen_path() -> PathBuf {
    work().join("tutoriel_vu") // le tutoriel a été ouvert une fois
}

pub fn habillage() -> Map<String, Value> {
    let mut h = match json!({"autoZoom": 1, "pop": false, "karaoke": false, "karaokeColor": "#ffd84d"}) {
        Value::Object(m) => m,
        _ => unreachable!(),
    };
    if let Some(Value::Object(m)) = util::read_json(habillage_path()) {
        for (k, v) in m {
            h.insert(k, v);
        }
    }
    h
}

pub fn save_habillage(body: &Map<String, Value>) -> Option<Map<String, Value>> {
    static COLOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#[0-9a-fA-F]{6}$").unwrap());
    let mut h = habillage();
    if let Some(v) = body.get("autoZoom") {
        h.insert("autoZoom".into(), util::num(util::round_to(py_float(v)?.clamp(0.0, 2.0), 2)));
    }
    for key in ["pop", "karaoke", "speakerFocus"] {
        if let Some(v) = body.get(key) {
            h.insert(key.into(), json!(util::truthy(Some(v))));
        }
    }
    if let Some(v) = body.get("karaokeColor") {
        let text = match v {
            Value::String(s) => s.clone(),
            other => pyjson::dumps(other),
        };
        if COLOR.is_match(&text) {
            h.insert("karaokeColor".into(), json!(text.to_lowercase()));
        }
    }
    util::mkdirs(work());
    let _ = write_json(habillage_path(), &Value::Object(h.clone()), Some(1), true);
    Some(h)
}

fn to_mp3(src: &Path, dst: &Path) {
    if let Some(d) = dst.parent() {
        util::mkdirs(d);
    }
    procs::call(
        paths::ffmpeg(),
        [
            "-v".as_ref(),
            "error".as_ref(),
            "-y".as_ref(),
            "-i".as_ref(),
            src.as_os_str(),
            "-c:a".as_ref(),
            "libmp3lame".as_ref(),
            "-b:a".as_ref(),
            "96k".as_ref(),
            dst.as_os_str(),
        ] as [&std::ffi::OsStr; 10],
    );
}

/// Page des moments avec le gabarit actuel (selection/page.html) et les données du rush
/// gardées dans work/moments/index.html : une mise à jour d'Auto-montage s'y voit tout de
/// suite, sans refaire la préparation.
pub fn moments_page() -> std::io::Result<String> {
    static DATA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^  const DATA = (\{.*\});$").unwrap());
    let html = fs::read_to_string(moments().join("index.html"))?;
    hf_preview_files(&moments()); // fichiers hf/ de l'aperçu des voix (mis à jour avec Auto-montage)
    let Some(m) = DATA.captures(&html) else { return Ok(html) };
    let Ok(Value::Object(mut data)) = serde_json::from_str::<Value>(&m[1]) else {
        return Err(std::io::Error::other("DATA illisible"));
    };
    // Sons personnels : ceux de la bibliothèque actuelle (ajoutés ou retirés depuis la dernière
    // génération), avec leur MP3 d'écoute dans la page.
    let sons: Vec<Value> = util::arr(&style_info(), "sons").to_vec();
    let son_ids: Vec<String> = sons.iter().filter_map(|s| s["id"].as_str().map(String::from)).collect();
    let id_of = |c: &Value| -> String {
        match c.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => pyjson::dumps(other),
        }
    };
    let old: Vec<Value> = data.get("sfxCatalog").and_then(Value::as_array).cloned().unwrap_or_default();
    let catalog: Vec<Value> = old.into_iter().filter(|c| !id_of(c).starts_with("perso-") || son_ids.contains(&id_of(c))).collect();
    // Sons par défaut ajoutés depuis la préparation de ce montage (nouvelle version) : dans le
    // catalogue, avec leur MP3 d'écoute, avant les sons personnels.
    let defaults = sound_catalog();
    let default_ids: Vec<Value> = defaults.iter().map(|c| c["id"].clone()).collect();
    let mut catalog: Vec<Value> =
        defaults.into_iter().chain(catalog.into_iter().filter(|c| !default_ids.contains(c.get("id").unwrap_or(&Value::Null)))).collect();
    data.insert(
        "voices".into(),
        json!(VOICES.iter().map(|(i, label, desc)| json!({"id": i, "label": label, "desc": desc})).collect::<Vec<_>>()),
    );
    for c in catalog.iter_mut() {
        // durée des sons personnels (calage des effets sur la page)
        let id = id_of(c);
        if id.starts_with("perso-") && !util::truthy(c.get("len")) {
            c["len"] = util::num(wav_len(&sounds_dir().join(format!("{}.wav", &id[6..]))));
        }
    }
    let sfx = paths::public().join("sfx");
    for (i, _, f, _) in DEFAULT_SOUNDS.iter() {
        let Some(f) = f else { continue };
        let mp3 = moments().join("sfx").join(format!("{i}.mp3"));
        if !mp3.exists() && sfx.join(f).exists() {
            to_mp3(&sfx.join(f), &mp3);
        }
    }
    for s in &sons {
        let sid = s["id"].as_str().unwrap_or("");
        let mp3 = moments().join("sfx").join(format!("{sid}.mp3"));
        if !mp3.exists() {
            to_mp3(&sounds_dir().join(format!("{}.wav", &sid[6..])), &mp3);
        }
        if !catalog.iter().any(|c| c.get("id").and_then(Value::as_str) == Some(sid)) {
            catalog.push(json!({"id": sid, "label": capitalize(s["label"].as_str().unwrap_or("")), "vol": 0.6}));
        }
    }
    data.insert("sfxCatalog".into(), Value::Array(catalog));
    data.insert("hiddenEffects".into(), json!(effects_hidden()));
    data.insert("sfxVolume".into(), util::num(effects_volume()));
    data.insert("habillage".into(), Value::Object(habillage()));
    data.insert("captionStyle".into(), caption_style_for_page());
    // Adresses des vidéos et vignettes suivies de leur date de modification : le navigateur
    // ne peut pas resservir la version de travail ou un extrait d'un rush ou d'un rendu
    // précédent (mêmes noms de fichiers d'une fois à l'autre).
    let fresh = |rel: Option<&Value>| -> Value {
        let Some(rel) = rel.filter(|r| util::truthy(Some(r))) else { return rel.cloned().unwrap_or(Value::Null) };
        let Some(text) = rel.as_str() else { return rel.clone() };
        match inside(&moments(), text) {
            Some(p) if p.is_file() => {
                let ms = util::mtime_ns(&p).unwrap_or(0) / 1_000_000;
                json!(format!("{text}?v={ms}"))
            }
            _ => rel.clone(),
        }
    };
    if let Some(Value::Object(w)) = data.get_mut("working") {
        let src = fresh(w.get("src"));
        w.insert("src".into(), src);
    }
    if let Some(Value::Array(list)) = data.get_mut("moments") {
        for mo in list.iter_mut() {
            if let Value::Object(mo) = mo {
                let (clip, thumb) = (fresh(mo.get("clip")), fresh(mo.get("thumb")));
                mo.insert("clip".into(), clip);
                mo.insert("thumb".into(), thumb);
            }
        }
    }
    let rush_name = match data.get("rush") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => "rush".into(),
    };
    let rush = splitext(&rush_name).0.to_string();
    // pas de « </script> » dans les données
    let text = pyjson::dumps_utf8(&Value::Object(data)).replace('<', "\\u003c");
    let tpl = fs::read_to_string(root().join("selection").join("page.html"))?;
    Ok(tpl.replace("__RUSH__", &html_escape(&rush)).replace("/*__DATA__*/null", &text))
}

/// Les liens vers les pages claude.ai pointent vers leurs équivalents locaux.
pub fn local_links(mut html: String) -> String {
    for (name, local) in [("moments_url.txt", "/"), ("downloads_url.txt", "/downloads/")] {
        if let Some(url) = read_text(work().join(name)).filter(|u| !u.is_empty()) {
            html = html.replace(&pyjson::dumps(&json!(url)), &pyjson::dumps(&json!(local)));
        }
    }
    html
}

pub fn instructions_path() -> PathBuf {
    work().join("instructions.md")
}
pub fn instructions_name_path() -> PathBuf {
    work().join("instructions.name")
}

pub fn instructions_info() -> Value {
    let path = instructions_path();
    if !path.exists() {
        return Value::Null;
    }
    let text = String::from_utf8_lossy(&fs::read(&path).unwrap_or_default()).into_owned();
    let preview: String = text.chars().take(600).collect();
    let name = read_text(instructions_name_path()).filter(|n| !n.is_empty()).unwrap_or_else(|| "instructions.md".into());
    json!({"name": name, "size": text.len(), "preview": preview})
}

// --- Téléchargements (sans découpe : les fichiers sont servis directement) -------
pub fn downloads_data() -> Value {
    let rush_txt = read_text(work().join("rush.txt")).filter(|r| !r.is_empty()).unwrap_or_else(|| "rush".into());
    let rush = splitext(basename(&rush_txt)).0.to_string();
    let apercu = out().join("montage_apercu.mp4");
    let edit = util::read_json(edit_json());
    let size = edit_size(edit.as_ref()); // format du rush (9:16, 16:9, 4:3…)
    let res = |w: i64| res_label(size.0, size.1, w); // « 720x1280 » en 9:16 : petit côté de w
    let sources = [
        ("720p", "Légère", res(720), moments().join("montage.mp4"), "/montage.mp4",
         "Copie compressée de la vidéo entière, pour la partager ou la regarder sur téléphone."),
        ("1080p", "1080p", res(1080), apercu.clone(), "/out/montage_apercu.mp4",
         "Rendu de l'aperçu, à la résolution de la composition."),
        ("4k", "4K", res(2160), out().join("montage_4k.mp4"), "/out/montage_4k.mp4", "Rendu final, pour la publication."),
    ];
    let mut versions = vec![];
    for (vid, label, res, path, url, note) in sources {
        if !path.exists() {
            continue;
        }
        let mt = util::mtime(&path).unwrap_or(0.0);
        let duration = if edit_json().exists() {
            match util::read_json(edit_json()).and_then(|e| e.get("durationInFrames").and_then(Value::as_f64)) {
                Some(d) => util::num(d / 30.0),
                None => json!(0),
            }
        } else {
            json!(0)
        };
        let stale = vid != "720p" && apercu.exists() && mt < util::mtime(&apercu).unwrap_or(0.0) - 60.0;
        versions.push(json!({
            "id": vid, "label": label, "res": res, "note": note, "parts": [url], "reencoded": null,
            "filename": format!("{rush}_{vid}_{}.mp4", local_fmt(mt, "%Y-%m-%d_%Hh%M")),
            "size": fs::metadata(&path).map(|m| m.len()).unwrap_or(0), "duration": duration, "renderedAt": iso_utc(mt),
            "stale": stale,
        }));
    }
    json!({"rush": rush, "versions": versions, "momentsPage": "/", "final": final_info()})
}

/// Création de la version 4K depuis les pages : possible, et durée estimée sur cette machine.
pub fn final_info() -> Value {
    let apercu = out().join("montage_apercu.mp4");
    let edit = util::read_json(edit_json());
    let parsed = edit.as_ref().and_then(|e| {
        let d = e.get("durationInFrames")?.as_f64()?;
        let fps = e.get("fps")?.as_f64()?;
        (fps != 0.0).then(|| (d / fps, edit_size(Some(e))))
    });
    let (montage, res) = match parsed {
        Some((m, (w, h))) => (m, res_label(w, h, 2160)),
        None => (0.0, "2160x3840".to_string()),
    };
    let rates = load_rates();
    let rush_rel = read_text(work().join("rush.txt")).filter(|r| !r.is_empty()).unwrap_or_else(|| "-".into());
    let rush_path = root().join(&rush_rel);
    let rush = media_seconds(&rush_path);
    let scale = |kind: &str| match kind {
        "montage" => montage,
        "rush" => rush,
        _ => 1.0,
    };
    let estimate: f64 = ["Source 4K", "Rendu 4K", "étape:rendu 4K"]
        .iter()
        .map(|k| rate(&rates, k) * scale(RATES.iter().find(|r| r.0 == *k).map(|r| r.1).unwrap_or("fixe")))
        .sum();
    // mémoire nécessaire (rendu 4K : le plus gourmand) comparée à la mémoire libre
    let names: Vec<String> = jobs::final_steps().into_iter().map(|(n, _)| n).collect();
    let mem = memoire::summary(&names, &memoire::video_info(&[rush_path], Some(montage)));
    let mem = json!({"peak": mem["peak"], "available": mem["available"], "level": mem["level"], "message": mem["message"]});
    let running = {
        let j = jobs::JOB.lock().unwrap();
        j.m["state"] == "running" && j.m["kind"] == "final"
    };
    json!({"available": apercu.exists() && montage > 0.0, "estimate": util::round_int(estimate), "res": res,
           "memory": mem, "running": running})
}

// --- Montages enregistrés et rushes (page « Rushes et montages ») -----------------------
/// project (montages enregistrés) : (réussi, message).
pub fn run_project(args: &[&str]) -> (bool, String) {
    let out = std::process::Command::new(paths::self_exe()).arg("project").args(args).current_dir(root()).output();
    match out {
        Ok(o) => {
            let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let last = text.trim().lines().last().unwrap_or("").to_string();
            (o.status.success(), last)
        }
        Err(e) => (false, e.to_string()),
    }
}

fn current_rush() -> String {
    basename(&read_text(work().join("rush.txt")).unwrap_or_default()).to_string()
}

pub fn projects_data() -> Value {
    let out = std::process::Command::new(paths::self_exe()).args(["project", "list"]).current_dir(root()).output();
    let saved = out.ok().and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok()).unwrap_or_else(|| json!([]));
    let current = current_rush();
    let names: Vec<String> = util::list_dir(moments().join("thumbs")).into_iter().filter(|n| n.starts_with("m_")).collect();
    json!({"current": if current.is_empty() { Value::Null } else { json!(current) },
           "currentPrepared": moments().join("index.html").exists(),
           "currentThumb": if names.is_empty() { Value::Null } else { json!(format!("/thumbs/{}", names[names.len() / 3])) },
           "saved": saved})
}

pub fn rush_list() -> Value {
    let current = current_rush();
    let mut names = util::list_dir(rushes());
    names.sort_by_key(|n| n.to_lowercase());
    let mut items = vec![];
    for name in names {
        let path = rushes().join(&name);
        if path.is_file() && is_video(&name) && !DERIVED.is_match(&name) && !name.starts_with('.') && !name.contains(".part.") {
            items.push(json!({"name": name, "size": fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                              "modified": iso_local(util::mtime(&path).unwrap_or(0.0))}));
        }
    }
    let langues: Map<String, Value> = moments_lib::LANGUAGES.iter().map(|(c, l)| (c.to_string(), json!(l))).collect();
    json!({"rushes": items, "current": if current.is_empty() { Value::Null } else { json!(current) },
           "prepared": moments().join("index.html").exists(),
           "free": disk_free(root()), "job": jobs::job_status(),
           "instructions": instructions_info(), "claude": super::claude::claude_connected(false), "style": style_info(),
           // Langue parlée (transcription) : choix de la prochaine préparation, langue du rush en cours.
           "langue": moments_lib::language(&work()),
           "langueRush": if current.is_empty() { Value::Null } else { json!(moments_lib::language_from(&work(), "langue_rush.txt")) },
           "langues": langues})
}

/// Message « pas assez de place » : place libre, place nécessaire et ce qui en prend le plus
/// dans Auto-montage (à supprimer pour en libérer).
pub fn no_space(need: u64, folder: &Path) -> String {
    let free = disk_free(folder);
    let gb = |n: u64| format!("{:.1} Go", n as f64 / (1u64 << 30) as f64);
    let mut parts = vec![
        (dir_size(&rushes()), "vidéos envoyées (montage/public/rushes)"),
        (dir_size(&work().join("projets")), "montages enregistrés (page Rushes et montages, « Supprimer »)"),
        (dir_size(&out()), "rendus (montage/out)"),
        (dir_size(&work().join("sauvegardes")), "sauvegardes (montage/work/sauvegardes)"),
    ];
    parts.sort_by(|a, b| b.cmp(a));
    let used: Vec<String> = parts.iter().filter(|(n, _)| *n >= 1 << 28).map(|(n, label)| format!("{label} {}", gb(*n))).collect();
    format!(
        "Pas assez de place sur le disque : il faut {} libres, il en reste {}.{} Pour en libérer : « Libérer de la place », en bas de la page « Rushes et montages ».",
        gb(need),
        gb(free),
        if used.is_empty() { String::new() } else { format!(" Dans Auto-montage : {}.", used.join(", ")) }
    )
}
