//! Interface web locale (Docker ou poste) : page des moments, vidéo entière et téléchargements.
//!
//! Usage : auto-montage local_server [--host 0.0.0.0] [--port 8080]
//!   /              page des moments (work/moments/index.html, générée par make_moments)
//!   /rush/         nouveau rush : choix du fichier (envoyé dans public/rushes/) et préparation
//!                  (prepare, rendu de la vidéo entière, make_moments) par boutons
//!   /downloads/    versions 720p, 1080p et 4K à télécharger (out/)
//!   /chat/         discussion avec Claude Code sur le montage (claude -p, outils limités au
//!                  projet, conversation reprise d'un message à l'autre, historique dans work/chat/)
//! Les pages sont les mêmes que sur claude.ai ; local/shim.js leur fournit en local ce que
//! claude.ai leur donne : enregistrement des sélections, corrections et effets dans
//! work/local_db/<collection>/<id>.json (même forme que l'export ArtifactData, lu tel quel par
//! use_selection), et téléchargement direct. « Générer la vidéo entière » lance ici
//! use_selection, build_edit, render apercu et make_moments, sans passer par Claude.
//!
//! Serveur à fils d'exécution (un par requête, comme ThreadingHTTPServer) : tiny_http.
//! Modules : common (dossiers, enveloppe des pages, chemins sûrs), info (données des pages),
//! jobs (tâches longues, pause et reprise, journal, mémoire), claude (connexion, discussion,
//! mises à jour), http (contrôles, fichiers, routes).

mod claude;
mod common;
mod http;
mod info;
mod jobs;

use std::io::Write;

pub fn main(args: &[String]) -> i32 {
    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = 8080;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let (key, inline) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut value = || -> Option<String> {
            if inline.is_some() {
                return inline.clone();
            }
            i += 1;
            args.get(i).cloned()
        };
        match key.as_str() {
            "--host" => match value() {
                Some(v) => host = v,
                None => return usage("--host demande une valeur"),
            },
            "--port" => match value().and_then(|v| v.parse().ok()) {
                Some(p) => port = p,
                None => return usage("--port demande un nombre"),
            },
            "-h" | "--help" => {
                println!("Usage : auto-montage local_server [--host HOST] [--port PORT]");
                return 0;
            }
            other => return usage(&format!("argument inconnu : {other}")),
        }
        i += 1;
    }
    let _ = *common::STARTED;
    let _ = &*common::VERSION;
    claude::chat_load(); // réponse coupée par un arrêt du serveur : marquée « Interrompu »
    jobs::close_interrupted_logs();
    jobs::remove_partial_files(true);
    std::thread::spawn(claude::update_loop);
    let addr = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
    crate::relance::cleanup(); // restes d'un remplacement du programme
    // Relance après une mise à jour : l'ancien serveur peut tenir le port quelques instants.
    let mut tries = 0;
    let server = loop {
        match tiny_http::Server::http(&addr) {
            Ok(s) => break s,
            Err(e) if tries < 40 => {
                tries += 1;
                if tries == 1 {
                    eprintln!("Auto-montage : {addr} occupé ({e}), nouvel essai…");
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Err(e) => {
                eprintln!("Auto-montage : impossible d'écouter sur {addr} ({e})");
                return 1;
            }
        }
    };
    println!("Auto-montage : http://localhost:{port}/ (moments) et /downloads/ (téléchargements)");
    let _ = std::io::stdout().flush();
    for request in server.incoming_requests() {
        std::thread::spawn(move || http::handle(request));
    }
    0
}

fn usage(msg: &str) -> i32 {
    eprintln!("Usage : auto-montage local_server [--host HOST] [--port PORT]\nlocal_server : erreur : {msg}");
    2
}
