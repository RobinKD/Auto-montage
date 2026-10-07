"""Version de travail du rush (public/rushes/rush_1080.mp4), encodée par morceaux d'une minute.

Format du rush, petit côté à 1080 (moments_lib.work_size), H.264 à image fixe, son du rush (toutes
ses pistes mélangées : moments_lib.audio_map). Chaque morceau est écrit dans work/reprise/travail/
(nom provisoire .part, renommé une fois complet) ; le son est extrait d'une traite en FLAC (un
son coupé en morceaux se décalerait de quelques centièmes de seconde à chaque reprise de la
lecture), puis les morceaux sont mis bout à bout et le son encodé en AAC. Une préparation mise en pause, arrêtée ou coupée reprend
au premier morceau manquant (mêmes rush, taille et réglages : work/reprise/travail/cle.txt).

Usage : python3 scripts/version_travail.py <rush>
"""
import os
import re
import shutil
import subprocess
import sys
from fractions import Fraction

import imageio_ffmpeg

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import progress  # noqa: E402
from moments_lib import audio_map, display_size, work_size  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
OUT = os.path.join(ROOT, "public", "rushes", "rush_1080.mp4")
DIR = os.path.join(ROOT, "work", "reprise", "travail")
FFMPEG = imageio_ffmpeg.get_ffmpeg_exe()
LABEL = "Version de travail (MP4)"
CHUNK = 60  # secondes par morceau (arrondi à un nombre entier d'images)
NTSC = {"23.98": Fraction(24000, 1001), "23.976": Fraction(24000, 1001), "29.97": Fraction(30000, 1001),
        "47.95": Fraction(48000, 1001), "59.94": Fraction(60000, 1001), "119.88": Fraction(120000, 1001)}


def probe(path):
    """(durée en secondes, images par seconde) de la vidéo, lues dans « ffmpeg -i »."""
    out = subprocess.run([FFMPEG, "-hide_banner", "-i", path], capture_output=True, text=True).stderr
    m = re.search(r"Duration: (\d+):(\d+):([\d.]+)", out)
    duration = int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3]) if m else 0.0
    video = re.search(r"Stream #\d+:\d+.*?: Video: (.*)", out)
    fps = None
    # Cadence nominale (« tbr », celle que ffmpeg garde pour une sortie à image fixe), sinon moyenne.
    for key in ("tbr", "fps"):
        r = video and re.search(r"([\d.]+)(k?) " + key, video[1])
        if r and not r[2]:
            fps = NTSC.get(r[1]) or Fraction(r[1]).limit_denominator(1000)
            if 1 <= fps <= 120:
                break
            fps = None
    return duration, fps or Fraction(30)


def main(rush):
    duration, fps = probe(rush)
    size = display_size(rush)
    w, h = work_size(*size) if size else (1080, 1920)
    amap = audio_map(rush)
    if not amap:
        sys.exit("Cette vidéo n'a pas de son.")
    frames = max(1, round(CHUNK * fps))  # morceaux d'un nombre entier d'images
    step = frames / fps
    # Dernier morceau : jusqu'à la fin (pas de morceau de moins d'une seconde).
    count = max(1, int(-(-(Fraction(duration).limit_denominator(1000) - 1) // step)))
    st = os.stat(rush)
    key = f"{os.path.basename(rush)} {st.st_size} {int(st.st_mtime)} {w}x{h} {fps} {frames} 2\n"
    os.makedirs(DIR, exist_ok=True)
    key_file = os.path.join(DIR, "cle.txt")
    if open(key_file).read() != key if os.path.exists(key_file) else True:
        shutil.rmtree(DIR)
        os.makedirs(DIR)
        open(key_file, "w").write(key)
    done = sum(os.path.exists(os.path.join(DIR, f"{i:04d}.mkv")) for i in range(count))
    if done:
        print(f"Reprise de la version de travail : {done} morceau(x) sur {count} déjà faits", flush=True)
    print(f"Version de travail {w}x{h}, {float(fps):.3f} images/s, {count} morceau(x)", flush=True)
    sound = os.path.join(DIR, "son.flac")
    if not os.path.exists(sound):
        if subprocess.run([FFMPEG, "-v", "error", "-y", "-i", rush, *amap, "-c:a", "flac", sound[:-5] + ".part.flac"]).returncode:
            sys.exit("Échec de l'extraction du son du rush")
        os.replace(sound[:-5] + ".part.flac", sound)
    for i in range(count):
        chunk = os.path.join(DIR, f"{i:04d}.mkv")
        start = float(i * step)
        length = float(step) if i < count - 1 else max(0.1, duration - start + 1)
        if os.path.exists(chunk):
            continue
        part = chunk[:-4] + ".part.mkv"
        code = progress.ffmpeg(LABEL, [
            FFMPEG, "-v", "error", "-y", "-ss", f"{start:.6f}", "-i", rush, "-t", f"{length:.6f}",
            "-map", "0:v:0", "-an", "-vf", f"scale={w}:{h},setsar=1", "-fps_mode", "cfr", "-r", str(fps),
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-g", "15", "-pix_fmt", "yuv420p", part], total=min(length, max(0.1, duration - start)), offset=start, whole=duration)
        if code:
            sys.exit(f"Échec de la version de travail (morceau {i + 1} sur {count})")
        os.replace(part, chunk)
    # Morceaux bout à bout (chacun dure exactement « step » secondes), son encodé en AAC.
    listing = os.path.join(DIR, "liste.txt")
    with open(listing, "w") as f:
        for i in range(count):
            f.write(f"file '{i:04d}.mkv'\n" + (f"duration {float(step):.6f}\n" if i < count - 1 else ""))
    part = OUT[:-4] + ".part.mp4"
    code = subprocess.run([FFMPEG, "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", listing, "-i", sound,
                           "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", part]).returncode
    if code:
        sys.exit("Échec de l'assemblage de la version de travail")
    os.replace(part, OUT)
    shutil.rmtree(DIR, ignore_errors=True)
    progress.report(LABEL, duration, duration, every=0)
    print(f"Version de travail prête : {os.path.relpath(OUT, ROOT)}", flush=True)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
