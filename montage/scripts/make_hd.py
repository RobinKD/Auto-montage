"""Source 4K pour le rendu final (le double de la version de travail : 2160x3840 en 9:16,
3840x2160 en 16:9, 2880x2160 en 4:3…), limitée aux images gardées au montage.

Au lieu d'encoder tout le rush en 4K, on ne garde que les plages « hdRanges » de
src/data/edit.json (clips + marge, calculées par build_edit.py), mises bout à bout dans
public/rushes/rush_2160.webm. Les images sont numérotées comme dans la version d'aperçu
(30 i/s constants), donc la correspondance avec trimBeforeHd est exacte.
Ne refait rien si les plages n'ont pas changé depuis le dernier encodage (work/hd_key.txt).
"""
import json
import os
import subprocess

import imageio_ffmpeg

from progress import ffmpeg as ffmpeg_with_progress

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
FFMPEG = imageio_ffmpeg.get_ffmpeg_exe()
OUT_VIDEO = os.path.join(ROOT, "public", "rushes", "rush_2160.webm")
KEY_FILE = os.path.join(ROOT, "work", "hd_key.txt")

edit = json.load(open(os.path.join(ROOT, "src", "data", "edit.json")))
rush = open(os.path.join(ROOT, "work", "rush.txt")).read().strip()

width, height = int(edit.get("width") or 1080), int(edit.get("height") or 1920)  # format du rush
ranges = edit["hdRanges"]  # calculées par build_edit.py
offset = sum(b - a for a, b in ranges)

key = f"{rush}|" + ",".join(f"{a}-{b}" for a, b in ranges)
previous = open(KEY_FILE).read() if os.path.exists(KEY_FILE) else ""
if previous == key and os.path.exists(OUT_VIDEO):
    print("Source 4K déjà à jour")
    raise SystemExit(0)

select = "+".join(f"between(n\\,{a}\\,{b - 1})" for a, b in ranges)
print(f"Encodage 4K de {offset} images ({offset / 30:.1f} s)…", flush=True)
PART = OUT_VIDEO.replace(".webm", ".part.webm")  # renommé une fois complet (« Annuler » possible)
code = ffmpeg_with_progress(
    "Source 4K",
    [
        FFMPEG, "-v", "error", "-y", "-i", os.path.join(ROOT, rush), "-map", "0:v:0", "-an",
        "-vf", f"scale={2 * width}:{2 * height},setsar=1,fps=30,select='{select}',settb=1/30,setpts=N",
        "-c:v", "libvpx-vp9", "-deadline", "good", "-cpu-used", "5", "-row-mt", "1",
        "-b:v", "0", "-crf", "20", "-g", "15", "-pix_fmt", "yuv420p", PART,
    ],
    total=offset / 30,
)
if code:
    raise SystemExit(f"Échec de l'encodage de la source 4K (ffmpeg, code {code})")
os.replace(PART, OUT_VIDEO)
open(KEY_FILE, "w").write(key)
print(f"Source 4K prête : {os.path.relpath(OUT_VIDEO, ROOT)}")
