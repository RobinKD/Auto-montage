//! Claude Code depuis l'interface : état de la connexion, connexion par un pseudo-terminal
//! (page /claude/), discussion (page /chat/), et vérification des mises à jour (page /updates/).

use super::common::*;
use crate::{paths, procs, pyjson, util};
use regex::Regex;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// PATH des programmes lancés pour Claude : le dossier d'auto-montage en tête, pour que les
/// commandes autorisées (« auto-montage build_edit »…) le trouvent.
pub fn claude_path_env() -> OsString {
    let mut dirs: Vec<PathBuf> = vec![];
    if let Some(d) = paths::self_exe().parent() {
        dirs.push(d.to_path_buf());
    }
    if let Some(p) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&p));
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// Commande « claude … » avec ce PATH.
pub fn claude_command() -> Command {
    let mut c = Command::new(paths::which("claude").unwrap_or_else(|| PathBuf::from("claude")));
    c.env("PATH", claude_path_env());
    c
}

/// Lance et attend au plus `timeout` : (code, sortie, erreurs). Err si trop long ou impossible.
pub fn run_timeout(mut cmd: Command, timeout: Duration) -> Result<(i32, String, String), String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let t_out = std::thread::spawn(move || {
        let mut s = vec![];
        let _ = out.read_to_end(&mut s);
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = vec![];
        let _ = err.read_to_end(&mut s);
        s
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let o = t_out.join().unwrap_or_default();
                let e = t_err.join().unwrap_or_default();
                return Ok((procs::py_code(&status), String::from_utf8_lossy(&o).into_owned(), String::from_utf8_lossy(&e).into_owned()));
            }
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} seconds", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

static CLAUDE_STATUS: Mutex<(Option<Instant>, bool)> = Mutex::new((None, false));

/// Claude Code utilisable sans interaction (connexion au compte claude.ai, jeton ou clé
/// d'API) : « claude auth status », gardé 30 s.
pub fn claude_connected(fresh: bool) -> bool {
    {
        let s = CLAUDE_STATUS.lock().unwrap_or_else(|e| e.into_inner());
        if let (false, Some(at)) = (fresh, s.0) {
            if at.elapsed() < Duration::from_secs(30) {
                return s.1;
            }
        }
    }
    let mut value = std::env::var("ANTHROPIC_API_KEY").map(|k| !k.is_empty()).unwrap_or(false);
    if !value && paths::which("claude").is_some() {
        let mut c = claude_command();
        c.args(["auth", "status", "--json"]);
        value = match run_timeout(c, Duration::from_secs(20)) {
            Ok((_, out, _)) => {
                let out = if out.trim().is_empty() { "{}".to_string() } else { out };
                match serde_json::from_str::<Value>(&out) {
                    Ok(v) => util::truthy(v.get("loggedIn")),
                    Err(_) => credentials_exist(),
                }
            }
            Err(_) => credentials_exist(),
        };
    }
    *CLAUDE_STATUS.lock().unwrap_or_else(|e| e.into_inner()) = (Some(Instant::now()), value);
    value
}

fn credentials_exist() -> bool {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    home.join(".claude").join(".credentials.json").exists()
}

/// Compte connecté (« claude auth status --text »), pour la page /claude/.
pub fn claude_account() -> String {
    if paths::which("claude").is_none() {
        return String::new();
    }
    let mut c = claude_command();
    c.args(["auth", "status", "--text"]);
    run_timeout(c, Duration::from_secs(20)).map(|(_, o, _)| o.trim().to_string()).unwrap_or_default()
}

// --- Mises à jour (page /updates/) ------------------------------------------------------
// Vérifiées au démarrage puis toutes les 6 h (auto-montage update check) ; « Mettre à jour »
// remplace les fichiers du programme (tâche « update »), puis le serveur s'arrête : le
// conteneur redémarre tout seul (restart: unless-stopped) avec la nouvelle version.
// « Réinitialiser » (tâche « reset ») retélécharge la version installée et remet les fichiers
// du programme modifiés ou effacés, puis redémarre de la même façon.
pub static UPDATE: Mutex<(f64, Option<Value>)> = Mutex::new((0.0, None));

pub fn update_check() -> Value {
    let mut c = Command::new(paths::self_exe());
    c.args(["update", "check"]).current_dir(root());
    let result = match run_timeout(c, Duration::from_secs(90)) {
        Ok((_, out, _)) => match serde_json::from_str::<Value>(&out) {
            Ok(v) => v,
            Err(e) => json!({"current": *VERSION, "error": format!("Vérification impossible ({e}).")}),
        },
        Err(e) => json!({"current": *VERSION, "error": format!("Vérification impossible ({e}).")}),
    };
    *UPDATE.lock().unwrap_or_else(|e| e.into_inner()) = (util::now_secs(), Some(result.clone()));
    result
}

pub fn update_loop() {
    std::thread::sleep(Duration::from_secs(60)); // laisse le démarrage se faire
    loop {
        update_check();
        std::thread::sleep(Duration::from_secs(6 * 3600));
    }
}

pub fn update_state() -> Value {
    let u = UPDATE.lock().unwrap_or_else(|e| e.into_inner());
    json!({"checkedAt": util::num(u.0), "result": u.1.clone().unwrap_or(Value::Null)})
}

pub fn update_summary() -> Value {
    let u = UPDATE.lock().unwrap_or_else(|e| e.into_inner());
    let r = u.1.clone().unwrap_or_else(|| json!({}));
    json!({"newer": util::truthy(r.get("newer")), "latest": r.get("latest").cloned().unwrap_or(Value::Null)})
}

// --- Connexion à Claude Code depuis l'interface (page /claude/) -----------------------
// « claude auth login » tourne dans un pseudo-terminal : il donne l'adresse de connexion
// (ouverte dans le navigateur de l'utilisateur, déjà connecté à claude.ai : il n'a qu'à
// autoriser), puis attend le code affiché après l'autorisation, que la page lui transmet.
struct Login {
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    output: String,
    url: Option<String>,
}

static LOGIN: LazyLock<Mutex<Login>> =
    LazyLock::new(|| Mutex::new(Login { child: None, master: None, writer: None, output: String::new(), url: None }));
static LOGIN_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https://[^\s\x1b\x07\]]*oauth/authorize[^\s\x1b\x07\]]*").unwrap());
static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[a-zA-Z]").unwrap());

fn login_lock() -> std::sync::MutexGuard<'static, Login> {
    LOGIN.lock().unwrap_or_else(|e| e.into_inner())
}

fn login_reader(mut reader: Box<dyn Read + Send>) {
    let mut buf = [0u8; 4096];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let text = ANSI.replace_all(&String::from_utf8_lossy(&buf[..n]), "").into_owned();
        let mut l = login_lock();
        l.output.push_str(&text);
        if l.output.len() > 20000 {
            let mut cut = l.output.len() - 20000;
            while !l.output.is_char_boundary(cut) {
                cut += 1;
            }
            l.output.drain(..cut);
        }
        if l.url.is_none() {
            if let Some(m) = LOGIN_URL.find(&l.output) {
                l.url = Some(m.as_str().to_string());
            }
        }
    }
}

fn running(l: &mut Login) -> bool {
    l.child.as_mut().map(|c| matches!(c.try_wait(), Ok(None))).unwrap_or(false)
}

fn stop(l: &mut Login) {
    if running(l) {
        if let Some(c) = l.child.as_mut() {
            #[cfg(unix)]
            {
                if let Some(pid) = c.process_id() {
                    procs::kill_group(pid, procs::Sig::Term);
                }
            }
            #[cfg(not(unix))]
            {
                let _ = c.kill();
            }
        }
    }
    l.writer = None;
    l.master = None;
    l.child = None;
}

pub fn login_pending() -> bool {
    running(&mut login_lock())
}

fn tail(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

/// Lance la connexion ; renvoie l'adresse à ouvrir (ou une erreur).
pub fn login_start(console: bool) -> Value {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    stop(&mut login_lock());
    let pair = match native_pty_system().openpty(PtySize { rows: 24, cols: 1000, pixel_width: 0, pixel_height: 0 }) {
        Ok(p) => p,
        Err(e) => return json!({"error": format!("Claude Code n'a pas donné d'adresse de connexion. ({e})"), "output": ""}),
    };
    let mut cmd = CommandBuilder::new(paths::which("claude").unwrap_or_else(|| PathBuf::from("claude")));
    cmd.args(["auth", "login"]);
    if console {
        cmd.arg("--console");
    }
    cmd.env("BROWSER", "true"); // pas de navigateur dans le conteneur
    cmd.env("PATH", claude_path_env());
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }
    let child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => return json!({"error": format!("Claude Code n'a pas donné d'adresse de connexion. ({e})"), "output": ""}),
    };
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().ok();
    let writer = pair.master.take_writer().ok();
    {
        let mut l = login_lock();
        l.child = Some(child);
        l.master = Some(pair.master);
        l.writer = writer;
        l.output.clear();
        l.url = None;
    }
    if let Some(r) = reader {
        std::thread::spawn(move || login_reader(r));
    }
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(250));
        let mut l = login_lock();
        if let Some(url) = &l.url {
            return json!({"url": url});
        }
        if !running(&mut l) {
            break;
        }
    }
    let l = login_lock();
    json!({"error": "Claude Code n'a pas donné d'adresse de connexion.", "output": tail(&l.output, 1500)})
}

/// Transmet le code d'autorisation ; renvoie l'état de la connexion.
pub fn login_code(code: &str) -> Value {
    {
        let mut l = login_lock();
        if !running(&mut l) || l.writer.is_none() {
            return json!({"error": "La connexion a expiré : recommencez depuis « Se connecter »."});
        }
        let code: String = code.replace(['\r', '\n'], "");
        let code = code.trim(); // une seule ligne : rien d'autre n'est tapé
        let w = l.writer.as_mut().unwrap();
        let _ = w.write_all(format!("{code}\r").as_bytes());
        let _ = w.flush();
    }
    for _ in 0..120 {
        std::thread::sleep(Duration::from_millis(250));
        if !running(&mut login_lock()) {
            break;
        }
    }
    // Réussi seulement si « claude auth login » s'est terminé sans erreur (un mauvais code le
    // fait échouer ou attendre) et que Claude Code se dit connecté.
    let exited_ok = {
        let mut l = login_lock();
        l.child.as_mut().and_then(|c| c.try_wait().ok().flatten()).map(|s| s.success()).unwrap_or(false)
    };
    let ok = exited_ok && claude_connected(true);
    let out = {
        let mut l = login_lock();
        let out = tail(&l.output, 1500);
        stop(&mut l);
        out
    };
    if ok {
        return json!({"ok": true});
    }
    let reason = out
        .lines()
        .rev()
        .find(|l| l.to_lowercase().contains("fail") || l.to_lowercase().contains("error"))
        .map(|l| l.trim().to_string())
        .unwrap_or_default();
    json!({"error": format!(
        "La connexion n'a pas abouti : vérifiez le code (il ne sert qu'une fois) et recommencez depuis « Ouvrir la page d'autorisation ».{}",
        if reason.is_empty() { String::new() } else { format!(" Détail : {}", tail(&reason, 200)) }
    ), "output": out})
}

pub fn logout() {
    let mut c = claude_command();
    c.args(["auth", "logout"]);
    let _ = run_timeout(c, Duration::from_secs(30));
    claude_connected(true);
}

// --- Discussion avec Claude Code (/chat/) -------------------------------------------------
// Chaque message lance « claude -p » depuis la racine du dépôt (CLAUDE.md chargé), en
// reprenant la conversation précédente (--resume). La réponse arrive en flux (stream-json) :
// textes et outils utilisés s'ajoutent au message en cours, que la page relit chaque seconde.
// Sans personne pour valider ses actions, Claude a des outils limités : lire et modifier les
// fichiers du projet, et lancer les seules commandes de montage (pas de commande libre ni de web).
// Pour le reste : lanceur « Claude Code » (terminal, chaque action validée).
fn chat_dir() -> PathBuf {
    work().join("chat")
}
fn chat_history() -> PathBuf {
    chat_dir().join("history.json")
}
fn chat_session() -> PathBuf {
    chat_dir().join("session.txt")
}

pub const CHAT_SCRIPTS: [&str; 7] = [
    "auto-montage use_selection work/local_db *",
    "auto-montage build_edit",
    "auto-montage render apercu",
    "auto-montage make_moments",
    "auto-montage make_downloads",
    "auto-montage decoupage *", // scinder ou fusionner des moments (puis make_moments)
    "auto-montage fonts_lib",   // polices utilisables (lecture seule)
];

pub fn chat_tools() -> Vec<String> {
    let mut v: Vec<String> = ["Read", "Glob", "Grep", "Edit(montage/work/**)", "Write(montage/work/**)"].iter().map(|s| s.to_string()).collect();
    v.extend(CHAT_SCRIPTS.iter().map(|c| format!("Bash(cd montage && {c})")));
    v
}

/// Interdits même dans montage/work/ : la conversation elle-même (son numéro est passé à
/// « claude --resume ») et les pages servies par l'interface (un texte piégé ne doit pas pouvoir
/// y glisser du code). Même liste pour apply_instructions et style_claude.
pub fn chat_denied() -> Vec<String> {
    let mut v = vec![];
    for t in ["Edit", "Write"] {
        for p in ["montage/work/chat/**", "montage/work/**/*.html", "montage/work/**/*.js"] {
            v.push(format!("{t}({p})"));
        }
    }
    v
}

static SESSION_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").unwrap());

pub fn chat_context() -> String {
    format!(
        "Tu es dans l'interface locale d'Auto-montage (conteneur Docker ou Podman) ; l'utilisateur
te parle depuis la page http://localhost:8080/chat/ et ne voit que tes textes et le nom des outils
utilisés. Tu peux lire le projet, modifier les fichiers de montage/work/ (suggestions.json,
edit_choices.json, caption_style.json pour le style des sous-titres, local_db/…) et lancer seulement ces commandes, écrites exactement ainsi :
{}
(* : nom de la variante, par exemple travail).
Tout autre outil ou commande est refusé : dans ce cas, dis à l'utilisateur d'ouvrir le lanceur
« Claude Code » (terminal où il valide chaque action). Pas de page claude.ai à publier, pas
d'ArtifactData ni de routine : la page des moments (http://localhost:8080/) est servie depuis
montage/work/moments/, les choix de la page sont dans montage/work/local_db/, et son bouton
« Générer la vidéo entière » fait use_selection, build_edit, render apercu et
make_moments. Tu ne peux pas poser de question à choix : demande en texte. Réponds en
français, simplement : l'utilisateur n'est pas développeur.",
        CHAT_SCRIPTS.iter().map(|c| format!("cd montage && {c}")).collect::<Vec<_>>().join("\n")
    )
}

struct Chat {
    running: bool,
    messages: Vec<Value>,
    pid: Option<u32>,
}

static CHAT: Mutex<Chat> = Mutex::new(Chat { running: false, messages: vec![], pid: None });

fn chat_lock() -> std::sync::MutexGuard<'static, Chat> {
    CHAT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Historique relu au démarrage ; une réponse coupée par un arrêt du serveur est marquée.
pub fn chat_load() {
    let mut c = chat_lock();
    c.messages = match util::read_json(chat_history()) {
        Some(Value::Array(a)) => a,
        _ => vec![],
    };
    for m in c.messages.iter_mut() {
        if m.get("role").and_then(Value::as_str) == Some("assistant") && !util::truthy(m.get("done")) {
            let mut parts = m.get("parts").and_then(Value::as_array).cloned().unwrap_or_default();
            parts.push(json!({"kind": "error", "text": "Interrompu (serveur redémarré)."}));
            m["done"] = json!(true);
            m["parts"] = Value::Array(parts);
        }
    }
}

fn chat_save(c: &Chat) {
    util::mkdirs(chat_dir());
    let tmp = chat_dir().join("history.json.tmp");
    let start = c.messages.len().saturating_sub(300);
    if write_json(&tmp, &Value::Array(c.messages[start..].to_vec()), Some(1), false).is_ok() {
        let _ = fs::rename(&tmp, chat_history());
    }
}

/// Résumé court d'un appel d'outil : commande, fichier ou motif.
fn tool_detail(data: Option<&Value>) -> String {
    let prefix = format!("{}{}", root().parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(), std::path::MAIN_SEPARATOR);
    for key in ["command", "file_path", "path", "pattern", "description"] {
        let Some(v) = data.and_then(|d| d.get(key)).filter(|v| util::truthy(Some(v))) else { continue };
        let text = match v {
            Value::String(s) => s.clone(),
            other => pyjson::dumps(other),
        }
        .replace(&prefix, "");
        let n = text.chars().count();
        return if n <= 160 { text } else { text.chars().take(157).collect::<String>() + "…" };
    }
    String::new()
}

fn parts_mut(reply: &mut Value) -> &mut Vec<Value> {
    if !reply.get("parts").map(Value::is_array).unwrap_or(false) {
        reply["parts"] = json!([]);
    }
    reply["parts"].as_array_mut().unwrap()
}

/// Ajoute à la réponse en cours ce qu'apporte un événement stream-json de claude -p.
fn chat_event(reply: &mut Value, event: &Value) {
    if let Some(sid) = event.get("session_id").filter(|v| util::truthy(Some(v))) {
        util::mkdirs(chat_dir());
        let text = match sid {
            Value::String(s) => s.clone(),
            other => pyjson::dumps(other),
        };
        let _ = fs::write(chat_session(), text);
    }
    match event.get("type").and_then(Value::as_str) {
        Some("assistant") => {
            for block in util::arr(event.get("message").unwrap_or(&Value::Null), "content") {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let t = block.get("text").and_then(Value::as_str).unwrap_or("").trim();
                        if !t.is_empty() {
                            parts_mut(reply).push(json!({"kind": "text", "text": t}));
                        }
                    }
                    Some("tool_use") => {
                        let name = block.get("name").cloned().unwrap_or(json!("outil"));
                        parts_mut(reply).push(json!({"kind": "tool", "name": name, "detail": tool_detail(block.get("input"))}));
                    }
                    _ => {}
                }
            }
        }
        Some("result") => {
            let as_text = |v: &Value| match v {
                Value::String(s) => s.clone(),
                other => pyjson::dumps(other),
            };
            let failed = event.get("subtype").map(|s| s != "success").unwrap_or(false);
            if util::truthy(event.get("is_error")) || failed {
                let text = event
                    .get("result")
                    .filter(|v| util::truthy(Some(v)))
                    .or_else(|| event.get("subtype").filter(|v| util::truthy(Some(v))))
                    .map(as_text)
                    .unwrap_or_else(|| "erreur".into());
                parts_mut(reply).push(json!({"kind": "error", "text": text}));
            } else if let Some(r) = event.get("result").filter(|v| util::truthy(Some(v))) {
                if !parts_mut(reply).iter().any(|p| p["kind"] == "text") {
                    parts_mut(reply).push(json!({"kind": "text", "text": as_text(r).trim()}));
                }
            }
        }
        _ => {}
    }
}

/// Erreurs de la discussion avec Claude, dans le journal (work/logs/discussion.log).
fn chat_log(text: &str) {
    util::mkdirs(logs());
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(logs().join("discussion.log")) {
        let _ = write!(f, "\n== {}\n{text}\n", now_fmt("%d/%m/%Y %H:%M:%S"));
    }
}

fn reply_push(part: Value) {
    let mut c = chat_lock();
    if let Some(r) = c.messages.last_mut() {
        parts_mut(r).push(part);
    }
}

fn run_chat(message: String, session: Option<String>) {
    // numéro de conversation abîmé : nouvelle conversation (jamais une option)
    let session = session.filter(|s| !s.is_empty() && SESSION_ID.is_match(s));
    // Le message passe par l'entrée standard : un texte commençant par « - » n'est pas une option.
    let mut cmd = claude_command();
    cmd.args(["-p", "--output-format", "stream-json", "--verbose", "--append-system-prompt"]).arg(chat_context());
    if let Some(s) = &session {
        cmd.args(["--resume", s]);
    }
    cmd.arg("--allowedTools").args(chat_tools()).arg("--disallowedTools").args(chat_denied());
    cmd.current_dir(root().parent().unwrap_or(root()));
    let result = (|| -> Result<(), String> {
        let (reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
        cmd.stdin(Stdio::piped()).stdout(writer.try_clone().map_err(|e| e.to_string())?).stderr(writer);
        procs::new_group(&mut cmd);
        let spawned = cmd.spawn();
        drop(cmd);
        let mut child = spawned.map_err(|e| e.to_string())?;
        chat_lock().pid = Some(child.id());
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(message.as_bytes());
        }
        let mut other: Vec<String> = vec![]; // lignes hors JSON : messages d'erreur de claude
        let mut r = std::io::BufReader::new(reader);
        let mut buf = vec![];
        loop {
            buf.clear();
            match std::io::BufRead::read_until(&mut r, b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&buf).trim().to_string();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(event) => {
                    if event.is_object() {
                        let mut c = chat_lock();
                        if let Some(reply) = c.messages.last_mut() {
                            chat_event(reply, &event);
                        }
                    }
                }
                Err(_) => {
                    other.push(line);
                    if other.len() > 20 {
                        other.remove(0);
                    }
                }
            }
        }
        let code = procs::wait(&mut child);
        let mut c = chat_lock();
        let Some(reply) = c.messages.last_mut() else { return Ok(()) };
        let has_error = util::arr(reply, "parts").iter().any(|p| p["kind"] == "error");
        if util::truthy(reply.get("stopped")) {
            parts_mut(reply).push(json!({"kind": "error", "text": "Arrêté."}));
        } else if code != 0 && !has_error {
            let mut text = other[other.len().saturating_sub(5)..].join("\n");
            if text.is_empty() {
                text = format!("Claude Code s'est arrêté (code {code}).");
            }
            if session.is_some() && text.contains("No conversation found") && chat_session().exists() {
                // conversation perdue (volume réinstallé) : repartir de zéro
                let _ = fs::remove_file(chat_session());
                text.push_str("\nLa conversation précédente est introuvable : renvoyez votre message.");
            }
            parts_mut(reply).push(json!({"kind": "error", "text": text}));
            let head: String = message.chars().take(300).collect();
            chat_log(&format!("Message : {head}\nCode {code}\n{}", other.join("\n")));
        } else if util::arr(reply, "parts").is_empty() {
            parts_mut(reply).push(json!({"kind": "error", "text": "Pas de réponse."}));
        }
        Ok(())
    })();
    if let Err(e) = result {
        reply_push(json!({"kind": "error", "text": e}));
        eprintln!("Discussion : {e}");
    }
    let mut c = chat_lock();
    if let Some(r) = c.messages.last_mut() {
        r["done"] = json!(true);
    }
    c.running = false;
    c.pid = None;
    chat_save(&c);
}

pub fn chat_send(message: String) -> Option<String> {
    {
        let mut c = chat_lock();
        if c.running {
            return Some("Claude répond encore : attendez la fin de sa réponse, ou arrêtez-la.".into());
        }
        let now = now_iso_secs();
        c.messages.push(json!({"role": "user", "text": message, "time": now}));
        c.messages.push(json!({"role": "assistant", "parts": [], "time": now, "done": false}));
        c.running = true;
        chat_save(&c);
    }
    let session = read_text(chat_session());
    std::thread::spawn(move || run_chat(message, session));
    None
}

pub fn chat_stop() {
    let pid = {
        let mut c = chat_lock();
        let Some(pid) = c.pid else { return };
        if let Some(m) = c.messages.last_mut() {
            m["stopped"] = json!(true);
        }
        pid
    };
    procs::kill_group(pid, procs::Sig::Term); // claude et les commandes qu'il a lancées
}

pub fn chat_reset() -> bool {
    let mut c = chat_lock();
    if c.running {
        return false;
    }
    c.messages.clear();
    for p in [chat_session(), chat_history()] {
        if p.exists() {
            let _ = fs::remove_file(p);
        }
    }
    true
}

pub fn chat_status() -> Value {
    let connected = claude_connected(false);
    let c = chat_lock();
    let start = c.messages.len().saturating_sub(200);
    json!({"running": c.running, "messages": c.messages[start..], "claude": connected,
           "installed": paths::which("claude").is_some()})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_events() {
        let mut reply = json!({"role": "assistant", "parts": [], "done": false});
        chat_event(&mut reply, &json!({"type": "assistant", "message": {"content": [{"type": "text", "text": " Bonjour "},
                                       {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}}]}}));
        chat_event(&mut reply, &json!({"type": "result", "subtype": "success", "result": "fini"}));
        assert_eq!(reply["parts"], json!([{"kind": "text", "text": "Bonjour"}, {"kind": "tool", "name": "Bash", "detail": "ls"}]));
        chat_event(&mut reply, &json!({"type": "result", "is_error": true, "result": "oups"}));
        assert_eq!(reply["parts"][2], json!({"kind": "error", "text": "oups"}));
    }

    #[test]
    fn tools_use_auto_montage() {
        assert!(chat_tools().contains(&"Bash(cd montage && auto-montage build_edit)".to_string()));
        assert_eq!(chat_denied().len(), 6);
    }
}
