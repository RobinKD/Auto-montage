"""Transcrit les segments de parole détectés par le VAD Silero, regroupés en morceaux.

Entrées : work/rush_16k.wav, work/vad.txt (sortie de vad-speech-segments)
Sortie  : work/segments.json -> [{id, start, end, text, words:[{w, a, b}]}]
Les temps sont en secondes, dans la timeline du rush original.

Whisper traite toujours une fenêtre de 30 s : une phrase de 2 s transcrite seule coûte autant
qu'une de 30 s. Les segments consécutifs sont donc mis bout à bout dans des morceaux de 28 s au
plus, séparés par 1 s de silence, et un seul Whisper (modèle chargé une fois) transcrit tous
les morceaux : 6 fois plus rapide sur un rush de 6 min. Chaque mot est rendu à son segment
d'après son temps DTW (alignement sur le son, plus juste que les temps des jetons, qui
débordent sur le segment voisin). Whisper omet souvent une hésitation isolée (« Euh… »,
« C'est… ») au milieu d'un morceau : les segments restés vides sont transcrits ensuite seuls,
comme avant, pour ne pas perdre les ratés que le dérush doit repérer.
"""
import json
import os
import re
import subprocess
import wave

from progress import report

ROOT = os.path.join(os.path.dirname(__file__), "..")
WORK = os.path.join(ROOT, "work")
WHISPER = os.environ.get("WHISPER_DIR") or os.path.join(ROOT, "whisper.cpp")  # Docker : /opt/whisper/whisper.cpp
PAD = 0.15  # son gardé autour de chaque segment
GAP = 1.0  # silence entre deux segments d'un morceau
CHUNK = 28.0  # longueur maximale d'un morceau (fenêtre de Whisper : 30 s)

with wave.open(os.path.join(WORK, "rush_16k.wav")) as w:
    sr = w.getframerate()
    audio = w.readframes(w.getnframes())
total = len(audio) / 2 / sr

segs = [
    (float(a) / 100, float(b) / 100)
    for a, b in re.findall(r"start = ([\d.]+), end = ([\d.]+)", open(os.path.join(WORK, "vad.txt")).read())
]

SEG_DIR = os.path.join(WORK, "seg")
os.makedirs(SEG_DIR, exist_ok=True)
for f in os.listdir(SEG_DIR):  # restes d'une préparation précédente
    os.remove(os.path.join(SEG_DIR, f))


def write_wav(path, data):
    with wave.open(path, "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(sr)
        out.writeframes(data)


# Morceaux : segments consécutifs (un segment plus long que CHUNK forme un morceau à lui seul).
# places : (début dans le morceau, fin dans le morceau, début dans le rush, id du segment).
chunks = []
cur, length = [], 0.0
for i, (s, e) in enumerate(segs):
    piece = min(total, e + PAD) - max(0, s - PAD)
    if cur and length + piece > CHUNK:
        chunks.append(cur)
        cur, length = [], 0.0
    cur.append(i)
    length += piece + GAP
if cur:
    chunks.append(cur)

jobs = []  # (fichier wav, places, secondes de parole)
for n, ids in enumerate(chunks):
    data, places = b"", []
    for i in ids:
        s, e = segs[i]
        cs, ce = max(0, s - PAD), min(total, e + PAD)
        pos = len(data) / 2 / sr
        data += audio[int(cs * sr) * 2 : int(ce * sr) * 2]
        places.append((pos, len(data) / 2 / sr, cs, i))
        data += b"\0\0" * int(GAP * sr)
    path = os.path.join(SEG_DIR, f"c{n:03d}.wav")
    write_wav(path, data)
    jobs.append((path, places, sum(segs[i][1] - segs[i][0] for i in ids)))


def whisper(files, label, weights, total_weight, done=0.0):
    """Un seul Whisper pour tous les fichiers (modèle chargé une fois) ; JSON complet à côté de
    chaque fichier (<fichier>.json), avec le temps DTW de chaque jeton. Avancement : poids du
    fichier (secondes de parole) ajouté quand son JSON est écrit."""
    cmd = [os.path.join(WHISPER, "build", "bin", "whisper-cli"),
           "-m", os.path.join(WHISPER, "ggml-large-v3-turbo.bin"),
           "-l", "fr", "-t", "4", "-mc", "0", "-ml", "1", "-sow", "-ojf", "-dtw", "large.v3.turbo"]
    for f in files:
        cmd += ["-f", f]
    proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, errors="replace")
    tail = []
    for line in proc.stderr:
        tail = (tail + [line.rstrip()])[-20:]
        m = re.search(r"saving output to '(.+)\.json'", line)
        if m and m.group(1) in weights:
            done += weights[m.group(1)]
            report(label, done, total_weight, every=0)
    if proc.wait():
        print("\n".join(tail), flush=True)
        raise SystemExit(f"Échec de la transcription (whisper-cli, code {proc.returncode})")
    return done


def words_of(path):
    """Mots de la sortie de Whisper : (texte, temps DTW du début, fin), en secondes du fichier."""
    out = []
    for t in json.load(open(path + ".json"))["transcription"]:
        tokens = [k for k in t.get("tokens", []) if not k["text"].startswith("[_")]
        text = t["text"].strip()
        if not text or not tokens:
            continue
        a = tokens[0].get("t_dtw", -1)
        a = a / 100 if a >= 0 else t["offsets"]["from"] / 1000
        out.append((text, a, t["offsets"]["to"] / 1000))
    return out


def place_words(found, places):
    """Range chaque mot dans le segment qui contient son début (le plus proche sinon) et
    convertit ses temps en temps du rush, bornés au segment."""
    by_seg = {}
    for text, a, b in found:
        pos, end, cs, i = min(places, key=lambda p: 0 if p[0] <= a <= p[1] else min(abs(a - p[0]), abs(a - p[1])))
        by_seg.setdefault(i, []).append([text, cs + a - pos, cs + b - pos])
    for i, ws in by_seg.items():
        s, e = segs[i]
        for k, w in enumerate(ws):
            w[1] = min(max(w[1], s, ws[k - 1][1] if k else s), e)
        for k, w in enumerate(ws):
            # Fin d'un mot : début du suivant ; pour le dernier, fin donnée par Whisper (ses
            # temps de jetons ne suivent pas le DTW : au moins 0,3 s), bornée au segment.
            w[2] = ws[k + 1][1] if k + 1 < len(ws) else min(e, max(w[2], w[1] + 0.3))
        by_seg[i] = [{"w": t, "a": round(a, 3), "b": round(b, 3)} for t, a, b in ws]
    return by_seg


# Avancement : secondes de parole transcrites (le temps de Whisper suit la longueur des morceaux).
speech = sum(e - s for s, e in segs) or 1
report("Transcription", 0, speech)
words = {}
if jobs:
    whisper([p for p, _, _ in jobs], "Transcription", {p: w for p, _, w in jobs}, speech)
    for path, places, _ in jobs:
        words.update(place_words(words_of(path), places))

# Segments restés vides : transcrits seuls (hésitation isolée, bout de phrase).
empty = [i for i in range(len(segs)) if not words.get(i)]
if empty:
    print(f"{len(empty)} segment(s) sans mot dans les morceaux : transcrits seuls", flush=True)
    alone = []
    for i in empty:
        s, e = segs[i]
        cs, ce = max(0, s - PAD), min(total, e + PAD)
        path = os.path.join(SEG_DIR, f"s{i:03d}.wav")
        write_wav(path, audio[int(cs * sr) * 2 : int(ce * sr) * 2])
        alone.append((path, [(0.0, ce - cs, cs, i)], e - s))
    extra = sum(w for _, _, w in alone)
    whisper([p for p, _, _ in alone], "Transcription", {p: w for p, _, w in alone}, extra)
    for path, places, _ in alone:
        words.update(place_words(words_of(path), places))

result = []
for i, (s, e) in enumerate(segs):
    ws = words.get(i, [])
    text = " ".join(x["w"] for x in ws)
    result.append({"id": i, "start": s, "end": e, "text": text, "words": ws})
    print(f"#{i} [{s:.2f}-{e:.2f}] {text}", flush=True)
report("Transcription", speech, speech, every=0)

json.dump(result, open(os.path.join(WORK, "segments.json"), "w"), ensure_ascii=False, indent=1)
