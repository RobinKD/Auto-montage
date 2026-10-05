"""Analyse des vidéos d'exemple (work/style/videos/), première étape : ce qui se mesure.

Pour chaque vidéo :
  - les coupes (changements de plan, filtre « scene » de ffmpeg) : rythme du montage ;
  - des images datées : régulièrement et juste après chaque coupe (planches de 4 x 3 images
    avec le temps écrit dessus), que Claude regarde ensuite (scripts/style_claude.sh) ;
  - les attaques sonores (montées brusques du volume hors parole continue) : bruitages
    probables, avec leur instant ;
  - la taille et la durée.
Sorties : work/style/analyse/faits.json et work/style/analyse/<vidéo>/planche-<n>.jpg.
Avancement : lignes « @progression » (scripts/progress.py).
"""
import json
import os
import re
import shutil
import subprocess
import sys

import cv2
import numpy as np

sys.path.insert(0, os.path.dirname(__file__))
from progress import report  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
VIDEOS = os.path.join(ROOT, "work", "style", "videos")
OUT = os.path.join(ROOT, "work", "style", "analyse")
VIDEO_EXT = (".mov", ".mp4", ".m4v", ".mkv", ".webm", ".avi", ".mts")
LABEL = "Analyse des vidéos d'exemple"
FRAMES_MAX = 36  # images par vidéo (3 planches de 12)


def ffmpeg_exe():
    import imageio_ffmpeg
    return imageio_ffmpeg.get_ffmpeg_exe()


def info(path):
    out = subprocess.run([ffmpeg_exe(), "-hide_banner", "-i", path], capture_output=True, text=True).stderr
    m = re.search(r"Duration: (\d+):(\d+):([\d.]+)", out)
    size = re.search(r"Video: .*?, (\d{2,5})x(\d{2,5})", out)
    rot = re.search(r"rotation of (-?[\d.]+) degrees", out)
    w, h = (int(size[1]), int(size[2])) if size else (0, 0)
    if rot and round(abs(float(rot[1]))) % 180 == 90:
        w, h = h, w
    return {"duration": int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3]) if m else 0.0, "width": w, "height": h}


def scene_cuts(path, duration, done_before, total):
    """Instants des changements de plan (score de scène > 0,3), avec l'avancement."""
    cmd = [ffmpeg_exe(), "-hide_banner", "-nostats", "-i", path, "-an",
           "-vf", "scale=320:-2,select='gt(scene,0.3)',showinfo", "-f", "null", "-", "-progress", "pipe:1"]
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    cuts = []

    def read_err():
        for line in proc.stderr:
            m = re.search(r"pts_time:([\d.]+)", line)
            if m:
                cuts.append(round(float(m[1]), 2))
    import threading
    t = threading.Thread(target=read_err, daemon=True)
    t.start()
    for line in proc.stdout:
        key, _, value = line.strip().partition("=")
        if key == "out_time_us" and value.isdigit():
            report(LABEL, done_before + 0.7 * min(duration, int(value) / 1e6), total)
    proc.wait()
    t.join(timeout=5)
    return sorted(set(cuts))


def frame_times(duration, cuts):
    """Images datées : régulières (environ 2 par seconde de plan) et juste après chaque coupe."""
    regular = [duration * (i + 0.5) / 18 for i in range(18)] if duration > 0 else [0]
    after = [min(duration - 0.05, c + 0.15) for c in cuts]
    times = sorted(set(round(t, 2) for t in regular + after if 0 <= t < max(duration, 0.1)))
    while len(times) > FRAMES_MAX:  # trop d'images : on en garde une sur deux, coupes d'abord
        times = sorted(set(round(t, 2) for t in after[:FRAMES_MAX // 2]) | set(times[::2]))[:FRAMES_MAX]
    return times


def grab(path, t, width=300):
    proc = subprocess.run([ffmpeg_exe(), "-v", "error", "-ss", f"{t:.2f}", "-i", path, "-frames:v", "1",
                           "-vf", f"scale={width}:-2", "-f", "image2pipe", "-vcodec", "png", "-"],
                          capture_output=True)
    if proc.returncode or not proc.stdout:
        return None
    return cv2.imdecode(np.frombuffer(proc.stdout, np.uint8), cv2.IMREAD_COLOR)


def label(img, text):
    cv2.rectangle(img, (0, 0), (img.shape[1], 26), (0, 0, 0), -1)
    cv2.putText(img, text, (6, 19), cv2.FONT_HERSHEY_SIMPLEX, 0.6, (255, 255, 255), 1, cv2.LINE_AA)
    return img


def sheets(frames, folder):
    """Planches de 4 x 3 images datées."""
    paths = []
    for n in range(0, len(frames), 12):
        chunk = frames[n:n + 12]
        h = max(f.shape[0] for _, f in chunk)
        w = chunk[0][1].shape[1]
        tiles = [label(cv2.resize(f, (w, h)), text) for text, f in chunk]
        while len(tiles) % 4:
            tiles.append(np.zeros((h, w, 3), np.uint8))
        rows = [np.hstack(tiles[i:i + 4]) for i in range(0, len(tiles), 4)]
        path = os.path.join(folder, f"planche-{n // 12 + 1}.jpg")
        cv2.imwrite(path, np.vstack(rows), [cv2.IMWRITE_JPEG_QUALITY, 82])
        paths.append(os.path.relpath(path, os.path.join(ROOT, "work")))
    return paths


def sound_onsets(path, duration):
    """Attaques sonores nettes (bruitages probables) : énergie sur 50 ms qui bondit au-dessus
    du niveau des 0,5 s précédentes."""
    proc = subprocess.run([ffmpeg_exe(), "-v", "error", "-i", path, "-vn", "-ac", "1", "-ar", "8000",
                           "-f", "s16le", "-"], capture_output=True)
    if proc.returncode or not proc.stdout:
        return []
    x = np.frombuffer(proc.stdout, np.int16).astype(np.float32) / 32768
    hop = 400  # 50 ms
    n = len(x) // hop
    if n < 20:
        return []
    e = np.sqrt((x[: n * hop].reshape(n, hop) ** 2).mean(axis=1)) + 1e-4
    out, last = [], -1.0
    for i in range(10, n):
        before = np.median(e[i - 10:i])
        t = i * hop / 8000
        if e[i] > 4 * before and e[i] > 0.05 and t - last > 0.4:
            out.append(round(t, 2))
            last = t
    return out[:60]


def main():
    names = sorted(n for n in os.listdir(VIDEOS) if n.lower().endswith(VIDEO_EXT) and not n.startswith(".")) \
        if os.path.isdir(VIDEOS) else []
    if not names:
        sys.exit("Aucune vidéo d'exemple (work/style/videos/).")
    if os.path.isdir(OUT):
        shutil.rmtree(OUT)
    os.makedirs(OUT)
    infos = {n: info(os.path.join(VIDEOS, n)) for n in names}
    total = sum(max(1.0, i["duration"]) for i in infos.values())
    done = 0.0
    report(LABEL, 0, total)
    facts = []
    for name in names:
        path, meta = os.path.join(VIDEOS, name), infos[name]
        dur = max(1.0, meta["duration"])
        print(f"{name} : {meta['duration']:.1f} s, {meta['width']}x{meta['height']}", flush=True)
        cuts = scene_cuts(path, dur, done, total)
        folder = os.path.join(OUT, re.sub(r"[^\w.-]+", "_", os.path.splitext(name)[0]))
        os.makedirs(folder, exist_ok=True)
        frames = []
        times = frame_times(meta["duration"], cuts)
        for k, t in enumerate(times):
            img = grab(path, t)
            if img is not None:
                frames.append((f"{t:.1f} s" + (" (coupe)" if any(abs(t - c - 0.15) < 0.02 for c in cuts) else ""), img))
            report(LABEL, done + dur * (0.7 + 0.25 * (k + 1) / len(times)), total)
        onsets = sound_onsets(path, meta["duration"])
        shots = len(cuts) + 1
        facts.append({
            "video": name, "duration": round(meta["duration"], 1), "size": f"{meta['width']}x{meta['height']}",
            "cuts": cuts, "shots": shots, "averageShot": round(meta["duration"] / shots, 2),
            "cutsPerMinute": round(len(cuts) * 60 / dur, 1), "soundOnsets": onsets,
            "sheets": sheets(frames, folder) if frames else [],
        })
        print(f"  {len(cuts)} coupes (un plan toutes les {meta['duration'] / shots:.1f} s), "
              f"{len(onsets)} attaques sonores, {len(frames)} images", flush=True)
        done += dur
        report(LABEL, done, total)
    json.dump({"videos": facts}, open(os.path.join(OUT, "faits.json"), "w"), ensure_ascii=False, indent=1)
    print(f"Faits mesurés : work/style/analyse/faits.json ({len(facts)} vidéo{'s' if len(facts) > 1 else ''})")


if __name__ == "__main__":
    main()
