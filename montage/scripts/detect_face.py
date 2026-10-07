"""Détecte la position du visage dans le rush (Haar cascade d'OpenCV).

Sortie : src/data/face.json -> [{t, x, y, r}] toutes les 0,5 s, en fractions
de l'image (1080x1920, ou 1920x1080 en paysage ; x, y = centre du visage, r = demi-largeur).
Modèle : work/models/haarcascade_frontalface_default.xml (dépôt opencv/opencv).
Les trous (visage caché par une main, etc.) sont comblés par interpolation.
Reprise : les détections sont enregistrées au fil de l'eau (work/reprise/visage.json) ; une
préparation mise en pause ou coupée (AM_REPRISE=1) repart de la dernière.
"""
import json
import os
import time

import cv2
import numpy as np

from progress import report

ROOT = os.path.join(os.path.dirname(__file__), "..")
SRC = os.path.join(ROOT, "public", "rushes", "rush_1080.mp4")
OUT = os.path.join(ROOT, "src", "data", "face.json")
STEP = 0.5

MODEL = os.path.join(ROOT, "work", "models", "haarcascade_frontalface_default.xml")
if not hasattr(cv2, "CascadeClassifier") or not os.path.exists(MODEL):
    # OpenCV sans détecteur Haar (version 5) ou modèle absent : zooms centrés sur l'image
    # plutôt qu'une préparation interrompue (pip3 install -r scripts/requirements.txt corrige).
    print(f"Détection du visage indisponible (OpenCV {cv2.__version__}) : zooms centrés sur l'image.")
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    import wave  # durée du rush : son 16 kHz écrit par prepare.sh juste avant
    with wave.open(os.path.join(ROOT, "work", "rush_16k.wav")) as w:
        duration = w.getnframes() / w.getframerate()
    json.dump([{"t": round(t, 2), "x": 0.5, "y": 0.4, "r": 0.2} for t in np.arange(0, duration + STEP, STEP)],
              open(OUT, "w"))
    raise SystemExit(0)
cascade = cv2.CascadeClassifier(MODEL)
cap = cv2.VideoCapture(SRC)
fps = cap.get(cv2.CAP_PROP_FPS)
n = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
W = cap.get(cv2.CAP_PROP_FRAME_WIDTH)
H = cap.get(cv2.CAP_PROP_FRAME_HEIGHT)
SW, SH = int(W) // 2 or 540, int(H) // 2 or 960  # image réduite de moitié pour la détection

CHECKPOINT = os.path.join(ROOT, "work", "reprise", "visage.json")
key = f"{n} {fps} {W}x{H} {os.path.getsize(SRC)}"
samples = []
try:
    saved = json.load(open(CHECKPOINT))
    if os.environ.get("AM_REPRISE") == "1" and saved["key"] == key:
        samples = [tuple(x) for x in saved["samples"]]
        print(f"Reprise de la position du visage à {samples[-1][0] if samples else 0:.0f} s", flush=True)
except (OSError, ValueError, KeyError, TypeError, IndexError):
    pass


def checkpoint():
    os.makedirs(os.path.dirname(CHECKPOINT), exist_ok=True)
    json.dump({"key": key, "samples": samples}, open(CHECKPOINT + ".part", "w"))
    os.replace(CHECKPOINT + ".part", CHECKPOINT)


saved_at = time.monotonic()
for t in np.arange(0, n / fps, STEP)[len(samples):]:
    report("Position du visage", t, n / fps)
    if time.monotonic() - saved_at > 20:
        checkpoint()
        saved_at = time.monotonic()
    cap.set(cv2.CAP_PROP_POS_FRAMES, int(t * fps))
    ok, frame = cap.read()
    if not ok:
        break
    small = cv2.resize(frame, (SW, SH))
    gray = cv2.cvtColor(small, cv2.COLOR_BGR2GRAY)
    faces = cascade.detectMultiScale(gray, 1.1, 6, minSize=(90, 90))
    if len(faces):
        x, y, w, h = max(faces, key=lambda f: f[2] * f[3])
        samples.append((t, (x + w / 2) / SW, (y + h / 2) / SH, w / 2 / SW))
    else:
        samples.append((t, None, None, None))
report("Position du visage", n / fps, n / fps, every=0)

known = [s for s in samples if s[1] is not None]
if not known:  # aucun visage trouvé (image sombre, visage de profil…) : zooms centrés
    known = [(0.0, 0.5, 0.4, 0.2)]
    print("Aucun visage détecté : zooms centrés sur l'image.")
ts = np.array([s[0] for s in known])
res = []
for t, x, y, r in samples:
    if x is None:
        x = float(np.interp(t, ts, [k[1] for k in known]))
        y = float(np.interp(t, ts, [k[2] for k in known]))
        r = float(np.interp(t, ts, [k[3] for k in known]))
    res.append({"t": round(float(t), 2), "x": round(x, 4), "y": round(y, 4), "r": round(r, 4)})

# Lissage léger pour éviter les à-coups.
for key in ("x", "y", "r"):
    v = np.array([p[key] for p in res])
    v = np.convolve(np.pad(v, 2, mode="edge"), np.ones(5) / 5, mode="valid")
    for p, val in zip(res, v):
        p[key] = round(float(val), 4)

os.makedirs(os.path.dirname(OUT), exist_ok=True)
json.dump(res, open(OUT, "w"))
if os.path.exists(CHECKPOINT):
    os.remove(CHECKPOINT)
print(f"{len(known)}/{len(samples)} détections, médiane x={np.median([k[1] for k in known]):.3f} y={np.median([k[2] for k in known]):.3f} r={np.median([k[3] for k in known]):.3f}")
