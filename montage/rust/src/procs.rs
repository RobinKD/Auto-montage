//! Lancement des programmes : sortie capturée, groupes de processus (annulation et pause d'une
//! tâche avec tout ce qu'elle a lancé), codes de sortie à la façon de Python (négatif = signal).

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

/// Code de sortie comme `Popen.returncode` : négatif si le programme a été tué par un signal.
pub fn py_code(status: &ExitStatus) -> i32 {
    if let Some(c) = status.code() {
        return c;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return -sig;
        }
    }
    -1
}

/// Lance et attend : (code, sortie, erreurs) en texte (UTF-8, caractères invalides remplacés).
pub fn run<I, S>(program: impl AsRef<OsStr>, args: I) -> (i32, String, String)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    match Command::new(program).args(args).stdin(Stdio::null()).output() {
        Ok(o) => (
            py_code(&o.status),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

/// Lance en héritant de la sortie (comme `subprocess.run(…, check=False)`), renvoie le code.
pub fn call<I, S>(program: impl AsRef<OsStr>, args: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    match Command::new(program).args(args).status() {
        Ok(s) => py_code(&s),
        Err(e) => {
            eprintln!("Impossible de lancer le programme : {e}");
            127
        }
    }
}

/// Sortie d'erreur de « ffmpeg -hide_banner -i <path> » (description des flux, durée).
pub fn ffmpeg_info(path: &Path) -> String {
    run(crate::paths::ffmpeg(), [OsStr::new("-hide_banner"), OsStr::new("-i"), path.as_os_str()]).2
}

/// Prépare une commande pour qu'elle tourne dans son propre groupe de processus : la tâche et tout
/// ce qu'elle lance s'arrêtent ensemble (kill_group).
pub fn new_group(cmd: &mut Command) -> &mut Command {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    cmd
}

/// Signaux envoyés à un groupe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)] // tous les signaux de procs::kill_group, même ceux pas encore employés
pub enum Sig {
    Term,
    Kill,
    Stop,
    Cont,
}

/// Envoie un signal à tout le groupe de processus lancé par new_group (pid = son chef).
/// Windows : arrêt de l'arbre (taskkill /T) ; pause et reprise non disponibles (false).
pub fn kill_group(pid: u32, sig: Sig) -> bool {
    #[cfg(unix)]
    {
        let s = match sig {
            Sig::Term => libc::SIGTERM,
            Sig::Kill => libc::SIGKILL,
            Sig::Stop => libc::SIGSTOP,
            Sig::Cont => libc::SIGCONT,
        };
        unsafe { libc::killpg(pid as libc::pid_t, s) == 0 }
    }
    #[cfg(windows)]
    {
        match sig {
            Sig::Term | Sig::Kill => Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false),
            _ => false,
        }
    }
}

/// Marque des programmes du rendu HyperFrames (variable d'environnement à 1, héritée par node, ses
/// navigateurs et ses ffmpeg) : les navigateurs ont leur propre session, hors du groupe de la tâche,
/// et survivent à node quand il est tué (manque de mémoire, arrêt forcé).
pub const RENDER_MARK: &str = "AM_RENDU_HYPERFRAMES";

/// Arrête (sans attendre) tous les programmes dont l'environnement contient `name=1`, sauf
/// celui-ci ; renvoie leur nombre.
pub fn kill_marked(name: &str) -> usize {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mark = std::ffi::OsString::from(format!("{name}=1"));
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_environ(UpdateKind::Always));
    let me = sysinfo::get_current_pid().ok();
    sys.processes()
        .iter()
        .filter(|(pid, p)| Some(**pid) != me && p.thread_kind().is_none() && p.environ().iter().any(|e| *e == mark))
        .filter(|(_, p)| p.kill())
        .count()
}

/// Attend la fin d'un enfant et renvoie son code à la façon de Python.
pub fn wait(child: &mut Child) -> i32 {
    child.wait().map(|s| py_code(&s)).unwrap_or(-1)
}

/// Nom d'un signal (« SIGKILL »…), pour expliquer un code de sortie.
pub fn signal_name(sig: i32) -> String {
    match sig {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => return sig.to_string(),
    }
    .to_string()
}
