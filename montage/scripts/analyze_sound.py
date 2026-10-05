"""Recherche des bruits parasites dans la version de travail (page des moments, « Son »).

Sur l'audio d'origine (work/rush_16k.wav), dans les parties gardées au montage
(src/data/edit.json) :
  - le bruit de fond : niveau des passages sans parole (souffle, ronflement, pièce) ;
  - les bruits parasites : sons très brefs qui ressortent nettement de ce qui les entoure
    (clics, claquements de bouche, chocs, coups sur le micro), avec leur instant dans la
    version de travail et dans le rush.
Sortie : work/son_analyse.json. Les bruits à atténuer et le nettoyage choisis sur la page
vont dans work/son.json, appliqués par build_edit.py.
"""
import json
import os
import subprocess
import wave

import numpy as np

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
WORK = os.path.join(ROOT, "work")
FPS = 30
HOP = 0.02  # trames de 20 ms


def load_audio():
    path = os.path.join(WORK, "rush_16k.wav")
    if not os.path.exists(path):
        import imageio_ffmpeg
        rush = os.path.join(ROOT, open(os.path.join(WORK, "rush.txt")).read().strip())
        subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-v", "error", "-y", "-i", rush, "-vn", "-ac", "1",
                        "-ar", "16000", "-c:a", "pcm_s16le", path], check=True)
    with wave.open(path) as w:
        sr = w.getframerate()
        x = np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32) / 32768
    return x, sr


def main():
    x, sr = load_audio()
    hop = int(sr * HOP)
    n = len(x) // hop
    db = 20 * np.log10(np.sqrt((x[: n * hop].reshape(n, hop) ** 2).mean(axis=1)) + 1e-6)
    segments = json.load(open(os.path.join(WORK, "segments.json")))
    edit = json.load(open(os.path.join(ROOT, "src", "data", "edit.json")))

    # Trames de parole (mots transcrits, avec une marge) : le reste est « hors parole ».
    speech = np.zeros(n, bool)
    for s in segments:
        for w in s.get("words", []):
            a, b = int((w["a"] - 0.08) / HOP), int((w["b"] + 0.08) / HOP)
            speech[max(0, a):max(0, min(n, b))] = True  # (pour le bruit de fond)

    # Parties gardées (temps du rush) et instant correspondant dans la version de travail.
    kept = [(c["trimBefore"] / FPS, (c["trimBefore"] + c["durationInFrames"]) / FPS, c["from"] / FPS, c["seg"])
            for c in edit["clips"]]
    in_edit = np.zeros(n, bool)
    for a, b, _, _ in kept:
        in_edit[int(a / HOP):min(n, int(b / HOP))] = True

    # Bruit de fond : niveau typique des passages sans parole (tout le rush).
    quiet = db[~speech]
    floor = float(np.percentile(quiet, 30)) if len(quiet) > 50 else float(np.percentile(db, 10))
    if floor < -60:
        level, advice = "très faible", 0
    elif floor < -50:
        level, advice = "faible", 1
    elif floor < -40:
        level, advice = "audible", 2
    else:
        level, advice = "fort", 3

    # Bruits parasites, dans les parties gardées : sons très brefs qui ressortent nettement de
    # ce qui les entoure (± 0,4 s), même pendant la parole :
    #   - clics, claquements de bouche, grésillements : aigus (3,5-8 kHz) ;
    #   - chocs, coups sur le micro, pied de table : graves (< 150 Hz) ;
    #   - bruits hors des mots (respiration forte, objet) : niveau global.
    win, step = 512, sr // 100  # analyse toutes les 10 ms
    frames = []
    for a, b, start, seg in kept:
        i0, i1 = int(a * sr), min(len(x) - win, int(b * sr))
        if i1 - i0 < win * 4:
            continue
        idx = np.arange(i0, i1, step)
        spec = np.abs(np.fft.rfft(np.stack([x[i:i + win] * np.hanning(win) for i in idx]), axis=1)) ** 2
        freqs = np.fft.rfftfreq(win, 1 / sr)
        hi = 10 * np.log10(spec[:, (freqs >= 3500)].sum(axis=1) + 1e-9)
        lo = 10 * np.log10(spec[:, (freqs < 150)].sum(axis=1) + 1e-9)
        k = 40  # ± 0,4 s
        for name, band, rise in (("clic", hi, 18), ("choc", lo, 18)):
            pad = np.pad(band, k, mode="edge")
            ref = np.array([np.median(pad[j:j + 2 * k + 1]) for j in range(len(band))])
            hot = band > ref + rise
            j = 0
            while j < len(hot):
                if not hot[j]:
                    j += 1
                    continue
                e = j
                while e < len(hot) and hot[e]:
                    e += 1
                # Isolé : 30 ms au plus, et nettement au-dessus de ce qui précède et suit de près
                # (une consonne monte et dure ; un clic retombe aussitôt).
                around = max(band[max(0, j - 4)], band[min(len(band) - 1, e + 3)])
                if e - j <= 3 and band[j:e].max() > around + 12:
                    t = (idx[j] - 0) / sr
                    frames.append((name, t, (e - j) * 0.01, float((band[j:e] - ref[j:e]).max()), start, a, seg))
                j = e
    events = []
    for name, t, dur, over, start, a, seg in frames:
        events.append({"src": [round(max(0.0, t - 0.02), 2), round(t + dur + 0.03, 2)],
                       "at": round(start + t - a, 2), "seg": seg, "dur": round(max(dur, 0.01), 2),
                       "peak": round(over, 1),
                       "kind": "clic ou claquement" if name == "clic" else "choc ou coup sur le micro"})
    # Un même bruit vu dans les deux bandes : gardé une fois (le plus marqué).
    events.sort(key=lambda e: -e["peak"])
    unique = []
    for e in events:
        if all(abs(e["at"] - u["at"]) > 0.15 for u in unique):
            unique.append(e)
    events = unique
    events.sort(key=lambda e: -e["peak"])
    events = sorted(events[:20], key=lambda e: e["at"])  # les 20 plus marqués
    out = {"floor": round(floor, 1), "level": level, "advice": advice, "events": events}
    json.dump(out, open(os.path.join(WORK, "son_analyse.json"), "w"), ensure_ascii=False, indent=1)
    print(f"Bruit de fond {level} ({floor:.0f} dB), {len(events)} bruits parasites possibles")


if __name__ == "__main__":
    main()
