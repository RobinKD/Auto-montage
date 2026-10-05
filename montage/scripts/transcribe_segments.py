"""Transcrit chaque segment de parole détecté par le VAD Silero séparément.

Entrées : work/rush_16k.wav, work/vad.txt (sortie de vad-speech-segments)
Sortie  : work/segments.json -> [{id, start, end, text, words:[{w, a, b}]}]
Les temps sont en secondes, dans la timeline du rush original.
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
PAD = 0.15

with wave.open(os.path.join(WORK, "rush_16k.wav")) as w:
    sr = w.getframerate()
    audio = w.readframes(w.getnframes())
total = len(audio) / 2 / sr

segs = [
    (float(a) / 100, float(b) / 100)
    for a, b in re.findall(r"start = ([\d.]+), end = ([\d.]+)", open(os.path.join(WORK, "vad.txt")).read())
]

os.makedirs(os.path.join(WORK, "seg"), exist_ok=True)
result = []
# Avancement : secondes de parole transcrites (le temps de Whisper suit la durée du segment).
speech = sum(e - s for s, e in segs) or 1
done = 0.0
report("Transcription", 0, speech)
for i, (s, e) in enumerate(segs):
    cs, ce = max(0, s - PAD), min(total, e + PAD)
    chunk = audio[int(cs * sr) * 2 : int(ce * sr) * 2]
    base = os.path.join(WORK, "seg", f"{i:03d}")
    with wave.open(base + ".wav", "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(sr)
        out.writeframes(chunk)
    subprocess.run(
        [
            os.path.join(WHISPER, "build", "bin", "whisper-cli"),
            "-m", os.path.join(WHISPER, "ggml-large-v3-turbo.bin"),
            "-f", base + ".wav", "-l", "fr", "-t", "4",
            "-mc", "0", "-ml", "1", "-sow", "-oj", "-of", base, "-np",
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    data = json.load(open(base + ".json"))
    words = []
    for t in data["transcription"]:
        txt = t["text"].strip()
        if not txt:
            continue
        a = cs + t["offsets"]["from"] / 1000
        b = cs + t["offsets"]["to"] / 1000
        words.append({"w": txt, "a": round(max(a, s), 3), "b": round(min(b, e), 3)})
    text = " ".join(x["w"] for x in words)
    result.append({"id": i, "start": s, "end": e, "text": text, "words": words})
    print(f"#{i} [{s:.2f}-{e:.2f}] {text}", flush=True)
    done += e - s
    report("Transcription", done, speech, every=0)

json.dump(result, open(os.path.join(WORK, "segments.json"), "w"), ensure_ascii=False, indent=1)
