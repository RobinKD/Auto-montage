//! Serveur MCP (Model Context Protocol) d'Auto-montage : Claude (Claude Code, Claude Desktop…)
//! pilote le montage avec des commandes simples, sans passer par la page.
//!
//! Usage : auto-montage mcp   (JSON-RPC 2.0 sur l'entrée et la sortie standard, un message par
//! ligne). Pour Claude Code, depuis montage/ : `claude mcp add auto-montage -- ./am mcp`.
//!
//! Les commandes passent par l'interface locale (http://127.0.0.1:8080, ou AM_URL), lancée au
//! besoin : mêmes règles, mêmes tâches et mêmes fichiers que la page des moments, qui voit les
//! changements tout de suite. Les lectures (moments, réglages, catalogue) lisent les fichiers.
//! Chaque modification faite ici est notée dans work/historique/mcp.json : « annuler » la défait
//! (document de moment ou de variante, réglage de tout le montage, scission ou fusion).

use crate::paths;
use crate::util;
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const PROTOCOL: &str = "2025-06-18";
const JOURNAL_KEEP: usize = 50;

fn base_url() -> String {
    std::env::var("AM_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into()).trim_end_matches('/').to_string()
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(timeout).build() // sans proxy : interface locale
}

/// Requête à l'interface locale : (code, JSON de la réponse).
fn http(method: &str, route: &str, body: Option<&Value>) -> Result<(u16, Value), String> {
    let url = format!("{}{route}", base_url());
    let req = agent(Duration::from_secs(120)).request(method, &url);
    let resp = match body {
        Some(b) => req.set("content-type", "application/json").send_string(&b.to_string()),
        None => req.call(),
    };
    let (code, text) = match resp {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(c, r)) => (c, r.into_string().unwrap_or_default()),
        Err(e) => return Err(format!("interface locale injoignable ({url}) : {e}")),
    };
    Ok((code, serde_json::from_str(&text).unwrap_or(Value::String(text))))
}

/// Réponse attendue (2xx), sinon le message d'erreur de l'interface.
fn call(method: &str, route: &str, body: Option<&Value>) -> Result<Value, String> {
    let (code, v) = http(method, route, body)?;
    if (200..300).contains(&code) {
        Ok(v)
    } else {
        Err(v.get("error").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("erreur {code} : {v}")))
    }
}

/// Interface locale lancée si elle ne répond pas (en arrière-plan, sans sortie : stdout est au MCP).
fn ensure_server() -> Result<(), String> {
    if http("GET", "/api/status", None).is_ok() {
        return Ok(());
    }
    let port = base_url().rsplit(':').next().and_then(|p| p.parse::<u16>().ok()).unwrap_or(8080);
    std::process::Command::new(paths::self_exe())
        .args(["local_server", "--port", &port.to_string()])
        .current_dir(paths::root())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("impossible de lancer l'interface locale : {e}"))?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(300));
        if http("GET", "/api/status", None).is_ok() {
            return Ok(());
        }
    }
    Err("l'interface locale ne répond pas (30 s)".into())
}

fn db() -> PathBuf {
    paths::work().join("local_db")
}
fn doc(coll: &str, id: &str) -> Option<Value> {
    util::read_json(db().join(coll).join(format!("{id}.json")))
}
fn rush() -> String {
    std::fs::read_to_string(paths::work().join("rush.txt"))
        .ok()
        .and_then(|t| std::path::Path::new(t.trim()).file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// Données de la page des moments (DATA de work/moments/index.html).
fn page_data() -> Result<Value, String> {
    let html = std::fs::read_to_string(paths::work().join("moments").join("index.html"))
        .map_err(|_| "pas de page des moments : préparez d'abord un rush (outil preparer_rush)".to_string())?;
    let line = html.lines().find_map(|l| l.strip_prefix("  const DATA = ")).ok_or("page des moments illisible")?;
    serde_json::from_str(line.trim_end_matches(';')).map_err(|e| format!("page des moments illisible : {e}"))
}

// --- Journal des modifications (« annuler ») -----------------------------------------------------
fn journal_path() -> PathBuf {
    paths::work().join("historique").join("mcp.json")
}
fn journal() -> Vec<Value> {
    util::read_json(journal_path()).and_then(|v| v.as_array().cloned()).unwrap_or_default()
}
fn note(entry: Value) {
    let mut j = journal();
    j.push(entry);
    let skip = j.len().saturating_sub(JOURNAL_KEEP);
    util::mkdirs(journal_path().parent().unwrap());
    let _ = util::write_json(journal_path(), &Value::Array(j[skip..].to_vec()), Some(1));
}

fn put_doc(coll: &str, id: &str, data: &Value, label: &str) -> Result<(), String> {
    let prev = doc(coll, id).unwrap_or(Value::Null);
    call("PUT", &format!("/api/db/{coll}/{id}"), Some(data))?;
    note(json!({"kind": "doc", "coll": coll, "id": id, "prev": prev, "label": label}));
    Ok(())
}

fn put_setting(route: &str, data: &Value, label: &str) -> Result<Value, String> {
    let prev = call("GET", route, None)?;
    let r = call("PUT", route, Some(data))?;
    let prev = if route == "/api/son" { prev.get("settings").cloned().unwrap_or(Value::Null) } else { prev };
    note(json!({"kind": "url", "url": route, "prev": prev, "label": label}));
    Ok(r)
}

// --- Outils ----------------------------------------------------------------------------------------
const SETTINGS: [(&str, &str, &str); 6] = [
    ("couleurs", "/api/couleur", "température, teinte, exposition, contraste, hautes, ombres, blancs, noirs, saturation, vibrance, look, intensite, vignette"),
    ("transitions", "/api/transitions", "transition entre les moments : kind (id du catalogue ou \"\"), dur (s)"),
    ("habillage", "/api/habillage", "autoZoom (0 à 2), pop, karaoke, karaokeColor, speakerFocus"),
    ("son", "/api/son", "voice (volume %), denoise 0-3, declick, highpass, clarity, loudness (LUFS ou false)"),
    ("sous_titres", "/api/caption-style", "police, graisse, taille, majuscules, couleur, lignes du style des sous-titres"),
    ("effets", "/api/effects", "effets proposés sur la page : hidden, volume"),
];

fn tools() -> Value {
    let id = json!({"type": "integer", "description": "id du moment (liste_moments)"});
    let setting_names: Vec<&str> = SETTINGS.iter().map(|s| s.0).collect();
    json!([
        {"name": "etat", "description": "État d'Auto-montage : rush en cours, mode, tâche en cours (préparation, génération) et son avancement, dernière vidéo générée.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "catalogue", "description": "Listes des choix possibles : effets visuels, effets d'image, transitions, looks de couleur, voix modifiées, sons, réglages de tout le montage.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "liste_moments", "description": "Moments du rush (id, début, fin, texte, gardé ou non dans la variante, réglages enregistrés). Les plans sans parole ont un id >= 1000.",
         "inputSchema": {"type": "object", "properties": {"variante": {"type": "string", "description": "variante (\"travail\" par défaut)"},
                                                          "avec_mots": {"type": "boolean", "description": "mots avec leurs instants (s depuis le début de l'extrait)"}}}},
        {"name": "garder_moments", "description": "Garde ou retire des moments de la vidéo entière (sélection de la variante).",
         "inputSchema": {"type": "object", "properties": {"garder": {"type": "array", "items": {"type": "integer"}}, "retirer": {"type": "array", "items": {"type": "integer"}},
                                                          "variante": {"type": "string"}}}},
        {"name": "regler_moment", "description": "Change les réglages d'un moment (fusionnés avec les existants ; null retire un réglage) : text (sous-titre corrigé), nosub, sfx, vfx (effets visuels, dont {\"kind\": \"filtre\", \"effect\", \"at\", \"dur\", \"force\"}), gain (volume de la voix, 1 = 100 %), focus (personne, \"auto\" ou \"tous\"), transition ({\"kind\", \"dur\"} avant ce moment), cuts, trim. Temps en s depuis le début de l'extrait du moment.",
         "inputSchema": {"type": "object", "required": ["id", "reglages"], "properties": {"id": id.clone(), "reglages": {"type": "object"}}}},
        {"name": "scinder_moment", "description": "Scinde un moment en deux avant le mot d'indice avant_mot (liste_moments avec_mots).",
         "inputSchema": {"type": "object", "required": ["id", "avant_mot"], "properties": {"id": id.clone(), "avant_mot": {"type": "integer"}}}},
        {"name": "fusionner_moments", "description": "Fusionne un moment avec les suivants (nombre = combien de fusions de suite, 1 par défaut).",
         "inputSchema": {"type": "object", "required": ["id"], "properties": {"id": id, "nombre": {"type": "integer"}}}},
        {"name": "lire_reglages", "description": "Réglages de tout le montage : couleurs, transitions, habillage, son, sous_titres (style), effets.",
         "inputSchema": {"type": "object", "required": ["quoi"], "properties": {"quoi": {"type": "string", "enum": setting_names}}}},
        {"name": "changer_reglages", "description": format!("Change des réglages de tout le montage (seules les clés données). {}",
            SETTINGS.iter().map(|s| format!("{} : {}", s.0, s.2)).collect::<Vec<_>>().join(" ; ")),
         "inputSchema": {"type": "object", "required": ["quoi", "valeurs"], "properties": {"quoi": {"type": "string", "enum": setting_names}, "valeurs": {"type": "object"}}}},
        {"name": "annuler", "description": "Défait la dernière modification faite par ces commandes (moment, sélection, réglage, scission ou fusion).",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "generer_video", "description": "Génère la vidéo entière (aperçu 1080p) d'après la variante, avec couleurs, effets, transitions, volume normalisé et sous-titres en fichiers. attendre : jusqu'à la fin (sinon suivre avec etat).",
         "inputSchema": {"type": "object", "properties": {"variante": {"type": "string"}, "attendre": {"type": "boolean"}, "delai_max": {"type": "integer", "description": "secondes (1800 par défaut)"}}}},
        {"name": "rendu_4k", "description": "Crée la version 4K de la vidéo entière générée (long).",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "sous_titres", "description": "Fichiers de sous-titres de la dernière vidéo générée (SRT et WebVTT) : chemins et contenu SRT.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "mesurer_volume", "description": "Mesure le volume d'un fichier vidéo ou son (EBU R128 : LUFS intégré, plage, crête vraie). Par défaut la dernière vidéo générée.",
         "inputSchema": {"type": "object", "properties": {"fichier": {"type": "string"}}}},
        {"name": "exporter_plateforme", "description": format!("Copies de la dernière vidéo générée pour des plateformes (format, débit, volume cible de chacune ; format différent du montage : image entière sur fond flou), dans out/export/. Sans plateformes : liste des préréglages et des exports faits. Préréglages : {}. couper : coupe à la durée maximale de la plateforme. attendre : jusqu'à la fin.",
            crate::export::PRESETS.iter().map(|p| format!("{} ({})", p.id, p.label)).collect::<Vec<_>>().join(", ")),
         "inputSchema": {"type": "object", "properties": {"plateformes": {"type": "array", "items": {"type": "string", "enum": crate::export::PRESETS.iter().map(|p| p.id).collect::<Vec<_>>()}},
                         "couper": {"type": "boolean"}, "attendre": {"type": "boolean"}, "delai_max": {"type": "integer", "description": "secondes (1800 par défaut)"}}}},
        {"name": "preparer_rush", "description": "Prépare un rush déjà dans montage/public/rushes/ (transcription, visages, locuteurs, page des moments). langue : fr ou en.",
         "inputSchema": {"type": "object", "required": ["rush"], "properties": {"rush": {"type": "string"}, "langue": {"type": "string", "enum": ["fr", "en"]}}}},
    ])
}

fn arg_str<'a>(a: &'a Map<String, Value>, k: &str, d: &'a str) -> &'a str {
    a.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(d)
}
fn safe(s: &str) -> Result<&str, String> {
    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        Ok(s)
    } else {
        Err(format!("nom invalide : {s}"))
    }
}

fn catalogue() -> Value {
    let js = std::fs::read_to_string(paths::root().join("selection").join("hf_look.js")).unwrap_or_default();
    let labels = |name: &str| -> Vec<Value> {
        let Some(start) = js.find(&format!("const {name} = [")) else { return vec![] };
        let end = start + js[start..].find("\n  ];").unwrap_or(0);
        let re = regex::Regex::new(r#"\{ id: "([a-z]+)", label: "([^"]+)""#).unwrap();
        re.captures_iter(&js[start..end]).map(|c| json!({"id": &c[1], "label": &c[2]})).collect()
    };
    json!({
        "effets_visuels": crate::moments_lib::VISUALS.iter().map(|(i, l)| json!({"id": i, "label": l})).collect::<Vec<_>>(),
        "effets_image": labels("EFFECTS"),
        "transitions": labels("TRANSITIONS"),
        "looks": labels("LOOKS"),
        "voix": crate::moments_lib::VOICES.iter().map(|(i, l, d)| json!({"id": i, "label": l, "desc": d})).collect::<Vec<_>>(),
        "sons": crate::moments_lib::SOUNDS.iter().map(|s| json!({"id": s.0, "label": s.1})).collect::<Vec<_>>(),
        "reglages": SETTINGS.iter().map(|s| json!({"quoi": s.0, "cles": s.2})).collect::<Vec<_>>(),
        "exemples": {
            "effet_image": {"kind": "filtre", "effect": "noirblanc", "at": 0.5, "dur": 2, "force": 100},
            "zoom": {"kind": "zoomin", "at": 0.3, "dur": 0.8, "force": 25},
            "son": {"kind": "whoosh", "at": 0.2},
            "transition": {"kind": "fondu", "dur": 0.5},
        },
    })
}

fn keep_of(variant: &str) -> Vec<Value> {
    doc("variantes", variant).map(|d| util::arr(&d, "keep").to_vec()).unwrap_or_default()
}

fn liste_moments(a: &Map<String, Value>) -> Result<Value, String> {
    let variant = safe(arg_str(a, "variante", "travail"))?;
    let data = page_data()?;
    let keep: Vec<String> = keep_of(variant).iter().map(|k| k.as_str().map(String::from).unwrap_or_else(|| k.to_string())).collect();
    let words = a.get("avec_mots").and_then(Value::as_bool).unwrap_or(false);
    let known = db().join("variantes").join(format!("{variant}.json")).exists();
    let mut out = vec![];
    for m in util::arr(&data, "moments") {
        let id = m.get("id").and_then(Value::as_i64).unwrap_or(-1);
        let kept = if known {
            keep.iter().any(|k| k == &id.to_string() || k.starts_with(&format!("{id}/")))
        } else {
            m.get("suggested").and_then(Value::as_bool).unwrap_or(false)
        };
        let mut o = json!({"id": id, "debut": m["start"], "fin": m["end"], "texte": m.get("text").cloned().unwrap_or(json!("")),
                           "garde": kept, "sans_parole": m.get("gap").and_then(Value::as_bool).unwrap_or(false)});
        if let Some(d) = doc("moments", &id.to_string()) {
            let set: Map<String, Value> = d.as_object().into_iter().flatten()
                .filter(|(k, v)| !v.is_null() && !["rush", "updatedAt"].contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone())).collect();
            if !set.is_empty() {
                o["reglages"] = Value::Object(set);
            }
        }
        if let Some(r) = m.get("reason").and_then(Value::as_str).filter(|r| !r.is_empty()) {
            o["raison"] = json!(r);
        }
        if words {
            o["mots"] = json!(util::arr(m, "words").iter().enumerate()
                .map(|(i, w)| json!({"i": i, "mot": w["w"], "a": w["a"]})).collect::<Vec<_>>());
        }
        out.push(o);
    }
    Ok(json!({"rush": data.get("rush").cloned().unwrap_or(Value::Null), "variante": variant,
              "variante_enregistree": known, "moments": out}))
}

fn garder(a: &Map<String, Value>) -> Result<Value, String> {
    let variant = safe(arg_str(a, "variante", "travail"))?.to_string();
    let ids = |k: &str| -> Vec<i64> { a.get(k).and_then(Value::as_array).map(|l| l.iter().filter_map(Value::as_i64).collect()).unwrap_or_default() };
    let (add, del) = (ids("garder"), ids("retirer"));
    let mut d = doc("variantes", &variant).unwrap_or_else(|| {
        // variante jamais enregistrée : les moments suggérés
        let keep: Vec<Value> = page_data().ok().map(|p| util::arr(&p, "moments").iter()
            .filter(|m| m.get("suggested").and_then(Value::as_bool).unwrap_or(false)).map(|m| m["id"].clone()).collect()).unwrap_or_default();
        json!({"name": variant, "rush": rush(), "keep": keep})
    });
    let token = |v: &Value| -> i64 { v.as_i64().or_else(|| v.as_str().and_then(|s| s.split('/').next()?.parse().ok())).unwrap_or(-1) };
    let mut keep: Vec<Value> = util::arr(&d, "keep").iter().filter(|k| !del.contains(&token(k))).cloned().collect();
    for id in add {
        if !keep.iter().any(|k| token(k) == id) {
            keep.push(json!(id));
        }
    }
    d["keep"] = Value::Array(keep.clone());
    d["updatedAt"] = json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    put_doc("variantes", &variant, &d, "sélection des moments")?;
    Ok(json!({"variante": variant, "gardes": keep.len(), "keep": keep}))
}

fn regler(a: &Map<String, Value>) -> Result<Value, String> {
    let id = a.get("id").and_then(Value::as_i64).ok_or("id manquant")?;
    let Some(Value::Object(patch)) = a.get("reglages") else { return Err("reglages : objet attendu".into()) };
    const FIELDS: [&str; 9] = ["text", "nosub", "sfx", "vfx", "gain", "focus", "transition", "cuts", "trim"];
    if let Some(k) = patch.keys().find(|k| !FIELDS.contains(&k.as_str())) {
        return Err(format!("réglage inconnu : {k} (possibles : {})", FIELDS.join(", ")));
    }
    if let Some(t) = patch.get("transition").filter(|t| !t.is_null()) {
        crate::look::transition_of(t).ok_or("transition inconnue (voir catalogue)")?;
    }
    for f in patch.get("vfx").and_then(Value::as_array).into_iter().flatten() {
        if f.get("kind").and_then(Value::as_str) == Some("filtre") && !crate::look::EFFECTS.contains(&f.get("effect").and_then(Value::as_str).unwrap_or("")) {
            return Err("effet d'image inconnu (voir catalogue : effets_image)".into());
        }
    }
    let mut d = doc("moments", &id.to_string()).unwrap_or_else(|| json!({}));
    let o = d.as_object_mut().ok_or("document illisible")?;
    o.insert("rush".into(), json!(rush()));
    for (k, v) in patch {
        o.insert(k.clone(), v.clone());
    }
    o.insert("updatedAt".into(), json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)));
    put_doc("moments", &id.to_string(), &d, &format!("réglages du moment {id}"))?;
    Ok(json!({"id": id, "document": d}))
}

/// Attend la fin de la tâche en cours de l'interface (délai en s).
fn wait_job(limit: u64) -> Value {
    let start = Instant::now();
    loop {
        let st = call("GET", "/api/job", None).unwrap_or(Value::Null);
        if st.get("state").and_then(Value::as_str) != Some("running") || start.elapsed().as_secs() >= limit {
            return st;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

fn job_summary(st: &Value) -> Value {
    json!({"tache": st.get("kind"), "etat": st.get("state"), "message": st.get("message"), "avancement": st.get("overall"),
           "reste_s": st.get("remaining"), "erreur": st.get("error"),
           "journal": util::arr(st, "log").iter().rev().take(8).rev().cloned().collect::<Vec<_>>()})
}

fn after_decoupage(r: Value, label: &str) -> Result<Value, String> {
    if let Some(s) = r.get("snapshot").and_then(Value::as_str) {
        note(json!({"kind": "snapshot", "name": s, "label": label}));
    }
    let st = wait_job(600);
    Ok(json!({"message": r.get("message"), "page": job_summary(&st)}))
}

fn annuler() -> Result<Value, String> {
    let mut j = journal();
    let Some(e) = j.pop() else { return Err("rien à annuler".into()) };
    let label = e.get("label").and_then(Value::as_str).unwrap_or("").to_string();
    match e.get("kind").and_then(Value::as_str) {
        Some("doc") => {
            let (coll, id) = (e["coll"].as_str().unwrap_or(""), e["id"].as_str().unwrap_or(""));
            let prev = if e["prev"].is_null() {
                // document créé ici : remis vide (aucun réglage)
                json!({"rush": rush(), "text": null, "nosub": null, "sfx": null, "vfx": null, "gain": null, "focus": null,
                       "transition": null, "trim": null, "cuts": null})
            } else {
                e["prev"].clone()
            };
            call("PUT", &format!("/api/db/{coll}/{id}"), Some(&prev))?;
        }
        Some("url") => {
            let url = e["url"].as_str().unwrap_or("");
            let mut prev = e["prev"].clone();
            if url == "/api/son" {
                // clés ajoutées depuis : retirées (null)
                if let (Some(p), Ok(cur)) = (prev.as_object_mut(), call("GET", url, None)) {
                    for k in cur.get("settings").and_then(Value::as_object).into_iter().flatten().map(|(k, _)| k.clone()) {
                        p.entry(k).or_insert(Value::Null);
                    }
                }
            }
            call("PUT", url, Some(&prev))?;
        }
        Some("snapshot") => {
            call("POST", "/api/moments/restore", Some(&json!({"snapshot": e["name"]})))?;
            wait_job(600);
        }
        _ => return Err("entrée du journal inconnue".into()),
    }
    util::mkdirs(journal_path().parent().unwrap());
    let _ = util::write_json(journal_path(), &Value::Array(j.clone()), Some(1));
    Ok(json!({"annule": label, "reste_a_annuler": j.len()}))
}

fn tool(name: &str, a: &Map<String, Value>) -> Result<Value, String> {
    match name {
        "catalogue" => return Ok(catalogue()),
        "liste_moments" => return liste_moments(a),
        "mesurer_volume" => {
            let f = a.get("fichier").and_then(Value::as_str).map(PathBuf::from).unwrap_or_else(|| paths::out().join("montage_apercu.mp4"));
            let s = crate::loudness::measure_file(&f)?;
            return Ok(json!({"fichier": f, "lufs": util::round_to(s.integrated, 1), "plage_lu": util::round_to(s.range, 1),
                             "crete_dbtp": util::round_to(s.true_peak, 1)}));
        }
        "sous_titres" => {
            let (srt, vtt) = (paths::out().join("montage.srt"), paths::out().join("montage.vtt"));
            if !srt.exists() {
                return Err("pas encore de sous-titres : générez la vidéo entière (generer_video)".into());
            }
            return Ok(json!({"srt": srt, "vtt": vtt, "entrees": crate::subtitles::count(&srt),
                             "contenu_srt": std::fs::read_to_string(&srt).unwrap_or_default()}));
        }
        _ => {}
    }
    ensure_server()?;
    match name {
        "etat" => {
            let st = call("GET", "/api/job", None)?;
            let out = paths::out().join("montage_apercu.mp4");
            Ok(json!({"rush": rush(), "mode": std::fs::read_to_string(paths::work().join("mode.txt")).unwrap_or_default().trim(),
                      "tache": job_summary(&st),
                      "video": if out.exists() { json!({"fichier": out, "date": util::mtime(&out)}) } else { Value::Null },
                      "interface": base_url()}))
        }
        "garder_moments" => garder(a),
        "regler_moment" => regler(a),
        "scinder_moment" => {
            let id = a.get("id").and_then(Value::as_i64).ok_or("id manquant")?;
            let i = a.get("avant_mot").and_then(Value::as_u64).ok_or("avant_mot manquant")? as usize;
            let data = page_data()?;
            let m = util::arr(&data, "moments").iter().find(|m| m["id"].as_i64() == Some(id)).cloned().ok_or("moment introuvable")?;
            let w = util::arr(&m, "words").get(i).cloned().ok_or("indice de mot hors du moment")?;
            let r = call("POST", "/api/moments/split", Some(&json!({"id": id, "t": w["a"]})))?;
            after_decoupage(r, &format!("scission du moment {id}"))
        }
        "fusionner_moments" => {
            let id = a.get("id").and_then(Value::as_i64).ok_or("id manquant")?;
            let n = a.get("nombre").and_then(Value::as_i64).unwrap_or(1);
            let r = call("POST", "/api/moments/merge", Some(&json!({"id": id, "count": n})))?;
            after_decoupage(r, &format!("fusion du moment {id}"))
        }
        "lire_reglages" | "changer_reglages" => {
            let quoi = arg_str(a, "quoi", "");
            let (_, route, _) = SETTINGS.iter().find(|s| s.0 == quoi).ok_or(format!("réglage inconnu : {quoi}"))?;
            if name == "lire_reglages" {
                return call("GET", route, None);
            }
            let v = a.get("valeurs").filter(|v| v.is_object()).ok_or("valeurs : objet attendu")?;
            put_setting(route, v, quoi)
        }
        "annuler" => annuler(),
        "generer_video" => {
            let variant = safe(arg_str(a, "variante", "travail"))?.to_string();
            let mut body = json!({"variant": variant});
            if doc("variantes", &variant).is_none() {
                if let Ok(Value::Object(l)) = liste_moments(&Map::new()) {
                    body["keep"] = json!(util::arr(&Value::Object(l), "moments").iter().filter(|m| m["garde"] == json!(true)).map(|m| m["id"].clone()).collect::<Vec<_>>());
                }
            }
            call("POST", "/api/regen", Some(&body))?;
            if a.get("attendre").and_then(Value::as_bool).unwrap_or(false) {
                let st = wait_job(a.get("delai_max").and_then(Value::as_u64).unwrap_or(1800));
                let out = paths::out().join("montage_apercu.mp4");
                return Ok(json!({"tache": job_summary(&st), "video": out, "volume": util::read_obj(paths::work().join("volume.json")).get("montage_apercu.mp4")}));
            }
            Ok(json!({"lance": true, "suivi": "outil etat"}))
        }
        "rendu_4k" => {
            call("POST", "/api/final", Some(&json!({})))?;
            Ok(json!({"lance": true, "suivi": "outil etat"}))
        }
        "exporter_plateforme" => {
            let presets = a.get("plateformes").and_then(Value::as_array).cloned().unwrap_or_default();
            if presets.is_empty() {
                return call("GET", "/api/export", None);
            }
            call("POST", "/api/export", Some(&json!({"presets": presets, "cut": a.get("couper").and_then(Value::as_bool).unwrap_or(false)})))?;
            if a.get("attendre").and_then(Value::as_bool).unwrap_or(false) {
                let st = wait_job(a.get("delai_max").and_then(Value::as_u64).unwrap_or(1800));
                let done = call("GET", "/api/export", None)?;
                return Ok(json!({"tache": job_summary(&st), "exports": done.get("exports")}));
            }
            Ok(json!({"lance": true, "suivi": "outil etat"}))
        }
        "preparer_rush" => {
            let r = call("POST", "/api/prepare", Some(&json!({"rush": arg_str(a, "rush", ""), "langue": arg_str(a, "langue", "fr")})))?;
            Ok(json!({"lance": true, "reponse": r, "suivi": "outil etat"}))
        }
        _ => Err(format!("outil inconnu : {name}")),
    }
}

fn reply(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}
fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Réponse à un message JSON-RPC (None : notification, sans réponse).
pub fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let id = id?; // notifications (initialized, cancelled…) : rien à répondre
    let params = msg.get("params").and_then(Value::as_object).cloned().unwrap_or_default();
    Some(match method {
        "initialize" => reply(&id, json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "auto-montage", "version": std::fs::read_to_string(paths::root().join("..").join("VERSION")).unwrap_or_default().trim()},
            "instructions": "Auto-montage : montage automatique de rushes face caméra. Commencer par etat et liste_moments ; catalogue donne les effets, transitions et looks possibles ; generer_video fait la vidéo entière. Les changements se voient aussi sur la page des moments (http://localhost:8080/).",
        })),
        "ping" => reply(&id, json!({})),
        "tools/list" => reply(&id, json!({"tools": tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").and_then(Value::as_object).cloned().unwrap_or_default();
            match tool(name, &args) {
                Ok(v) => reply(&id, json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}],
                                           "structuredContent": if v.is_object() { v.clone() } else { json!({"resultat": v}) }})),
                Err(e) => reply(&id, json!({"content": [{"type": "text", "text": e}], "isError": true})),
            }
        }
        _ => error(&id, -32601, &format!("méthode inconnue : {method}")),
    })
}

pub fn main(_args: &[String]) -> i32 {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let r: Vec<Value> = batch.iter().filter_map(handle).collect();
                if r.is_empty() { None } else { Some(Value::Array(r)) }
            }
            Ok(msg) => handle(&msg),
            Err(e) => Some(error(&Value::Null, -32700, &format!("JSON illisible : {e}"))),
        };
        if let Some(a) = answer {
            let _ = writeln!(out, "{a}");
            let _ = out.flush();
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocole() {
        let init = handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}})).unwrap();
        assert_eq!(init["result"]["protocolVersion"], json!("2025-03-26"));
        assert!(handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
        let list = handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
        let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"regler_moment") && names.contains(&"generer_video") && names.contains(&"annuler") && names.contains(&"exporter_plateforme"));
        let cat = handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "catalogue"}})).unwrap();
        let s = &cat["result"]["structuredContent"];
        assert_eq!(s["transitions"].as_array().unwrap().len(), crate::look::TRANSITIONS.len());
        assert_eq!(s["effets_image"].as_array().unwrap().len(), crate::look::EFFECTS.len());
        let bad = handle(&json!({"jsonrpc": "2.0", "id": 4, "method": "nimporte"})).unwrap();
        assert_eq!(bad["error"]["code"], json!(-32601));
    }
}
