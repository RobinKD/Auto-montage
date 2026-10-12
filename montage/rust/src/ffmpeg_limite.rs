//! ffmpeg donné à HyperFrames (render_hyperframes, HYPERFRAMES_FFMPEG_PATH) : HyperFrames extrait
//! les images de chaque plan avec un ffmpeg par plan, tous lancés en même temps (un montage de
//! 150 plans : 150 ffmpeg, plus de 10 Go de mémoire). Ces extractions (sortie « frame_%05d »)
//! passent ici une par place libre, AM_FFMPEG_SLOTS à la fois (verrous de fichiers dans
//! AM_FFMPEG_LOCKS) ; les autres appels (encodage, son, vérifications) vont directement à ffmpeg
//! (AM_FFMPEG_REAL).
//!
//! Le programme auto-montage fait ce travail quand il est lancé sous le nom « ffmpeg_limite »
//! (copie ou lien physique work/rendu_hyperframes/bin/ffmpeg_limite, fait par render_hyperframes)
//! ou par « auto-montage ffmpeg_limite <arguments de ffmpeg> ».
//!
//! Unix : le vrai ffmpeg remplace ce programme (exec), comme dans la version bash : HyperFrames
//! peut l'arrêter directement, et le verrou (descripteur hérité) reste pris jusqu'à sa fin.
//! Windows : ffmpeg est lancé et attendu, verrou gardé pendant ce temps, puis son code est rendu.

use fs4::fs_std::FileExt;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

pub fn main(args: &[String]) -> i32 {
    let Some(ff) = std::env::var_os("AM_FFMPEG_REAL").filter(|v| !v.is_empty()) else {
        eprintln!("ffmpeg_limite : AM_FFMPEG_REAL manquant");
        return 1;
    };
    let raw: Vec<OsString> = args.iter().map(OsString::from).collect();
    let joined = format!(" {} ", args.join(" "));
    if !joined.contains("frame_%05d") {
        return run(&ff, &raw, None);
    }
    let locks = std::env::var_os("AM_FFMPEG_LOCKS").filter(|v| !v.is_empty());
    let Some(locks) = locks else { return run(&ff, &raw, None) };
    let locks = PathBuf::from(locks);
    let _ = fs::create_dir_all(&locks);
    let slots: usize = std::env::var("AM_FFMPEG_SLOTS").ok().and_then(|s| s.trim().parse().ok()).unwrap_or(2);
    loop {
        for i in 1..=slots {
            let path = locks.join(format!("{i}.lock"));
            let Ok(file) = OpenOptions::new().create(true).write(true).truncate(false).open(&path) else { continue };
            // Verrou gardé par ffmpeg jusqu'à sa fin.
            if let Ok(true) = file.try_lock_exclusive() {
                return run(&ff, &raw, Some(file));
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Lance le vrai ffmpeg avec les mêmes arguments (le verrou, s'il y en a un, reste pris).
fn run(ff: &std::ffi::OsStr, args: &[OsString], lock: Option<File>) -> i32 {
    let mut cmd = Command::new(ff);
    cmd.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        use std::os::unix::process::CommandExt;
        if let Some(f) = &lock {
            // Le descripteur du verrou passe à ffmpeg (sans FD_CLOEXEC) : le verrou dure autant que lui.
            let fd = f.as_raw_fd();
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags >= 0 {
                    libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
                }
            }
        }
        let err = cmd.exec();
        eprintln!("ffmpeg_limite : {} ne se lance pas : {err}", ff.to_string_lossy());
        drop(lock);
        127
    }
    #[cfg(not(unix))]
    {
        let code = match cmd.status() {
            Ok(s) => s.code().unwrap_or(1),
            Err(e) => {
                eprintln!("ffmpeg_limite : {} ne se lance pas : {e}", ff.to_string_lossy());
                127
            }
        };
        drop(lock);
        code
    }
}
