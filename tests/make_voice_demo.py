"""Échantillons des voix modifiées (page « Rushes et montages », effet « Voix modifiée ») : la même
phrase, dite par une voix de synthèse neuronale (Piper, voix française « upmc »), d'abord normale
puis avec chaque voix modifiée du montage (moments_lib.voice_filter, force 100 %).
Usage : python3 tests/make_voice_demo.py <dossier de sortie>   (ex. montage/local/tutoriel/voix)
  -> <voix>.mp3 pour chaque voix de moments_lib.VOICES.
Besoin : pip install piper-tts ; le modèle de voix (63 Mo, huggingface.co) est téléchargé dans
~/.cache/auto-montage/ s'il manque.
"""
import os
import subprocess
import sys
import tempfile
import urllib.request
import wave

import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "montage", "scripts"))
from moments_lib import VOICES, batterie_warp, voice_filter  # noqa: E402

PHRASE = "Voici un outil de montage pratique."
MODEL = "fr_FR-upmc-medium"
MODEL_URL = f"https://huggingface.co/rhasspy/piper-voices/resolve/main/fr/fr_FR/upmc/medium/{MODEL}.onnx"
SR = 48000
# Voix « normale » : un peu plus posée, avec une pièce légère (moins « studio »).
NATURAL = "atempo=0.97,aecho=0.8:0.5:28:0.12,highpass=f=80,loudnorm=I=-18:TP=-2"


def ffmpeg():
    try:
        import imageio_ffmpeg
        return imageio_ffmpeg.get_ffmpeg_exe()
    except ImportError:
        return "ffmpeg"


def model_path():
    cache = os.path.join(os.path.expanduser("~"), ".cache", "auto-montage")
    os.makedirs(cache, exist_ok=True)
    for ext in (".onnx", ".onnx.json"):
        path = os.path.join(cache, MODEL + ext)
        if not os.path.exists(path):
            print(f"Téléchargement de {MODEL}{ext}…", flush=True)
            urllib.request.urlretrieve(MODEL_URL.replace(".onnx", ext), path)
    return os.path.join(cache, MODEL + ".onnx")


def read(path):
    with wave.open(path) as w:
        return np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32) / 32768


def write(path, x):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes((np.clip(x, -1, 1) * 32767).astype(np.int16).tobytes())


def loudness(path):
    out = subprocess.run([ffmpeg(), "-i", path, "-af", "volumedetect", "-f", "null", "-"],
                         capture_output=True, text=True).stderr
    return float(out.split("mean_volume:")[1].split("dB")[0])


def main():
    out_dir = sys.argv[1]
    os.makedirs(out_dir, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        raw, normal = os.path.join(tmp, "raw.wav"), os.path.join(tmp, "normal.wav")
        subprocess.run([sys.executable, "-m", "piper", "-m", model_path(), "-f", raw], input=PHRASE.encode(),
                       check=True, capture_output=True)
        subprocess.run([ffmpeg(), "-v", "error", "-y", "-i", raw, "-af", NATURAL, "-ar", str(SR), "-ac", "1", normal],
                       check=True)
        dry = read(normal)
        for voice, label, _ in VOICES:
            src, wet = os.path.join(tmp, "src.wav"), os.path.join(tmp, "wet.wav")
            graph = voice_filter(voice, 1.0)
            if voice == "batterie":  # comme build_edit.py : ralenti, puis remis à la durée d'origine
                slow = batterie_warp(dry, 1.0)
                write(src, slow)
                graph = graph.replace("[0:a]", f"[0:a]rubberband=tempo={len(slow) / len(dry):.4f},", 1)
            else:
                write(src, dry)
            subprocess.run([ffmpeg(), "-v", "error", "-y", "-i", src, "-filter_complex", graph, "-map", "[r]",
                            "-ar", str(SR), "-ac", "1", wet], check=True)
            gain = 10 ** ((-18 - loudness(wet)) / 20)  # même volume que la voix normale
            effect = read(wet) * gain
            gap = lambda s: np.zeros(int(SR * s), np.float32)
            write(src, np.concatenate([gap(0.2), dry, gap(0.7), effect, gap(0.3)]))
            subprocess.run([ffmpeg(), "-v", "error", "-y", "-i", src, "-af", "alimiter=limit=0.9:level=false",
                            "-c:a", "libmp3lame", "-q:a", "4", os.path.join(out_dir, f"{voice}.mp3")], check=True)
            print(f"{label} : {voice}.mp3", flush=True)


if __name__ == "__main__":
    main()
