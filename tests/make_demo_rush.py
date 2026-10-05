"""Rush de démonstration pour le tutoriel (captures et petits rendus de montage/local/tutoriel/) :
un personnage dessiné, face caméra, dont la bouche suit une voix de synthèse (espeak-ng),
avec des blancs et une phrase reprise (1re prise à écarter), en 1080x1920.
Usage : python3 tests/make_demo_rush.py <sortie.mov>   (besoin : espeak-ng, ffmpeg, numpy, opencv)
Pas de vraie personne : les images du tutoriel peuvent être publiées sans souci de droit à l'image.
"""
import os
import subprocess
import sys
import tempfile
import wave

import cv2
import numpy as np

W, H, FPS, SR = 1080, 1920, 30, 48000
PHRASES = [
    "Salut ! Aujourd'hui, trois astuces pour faire des économies sans te priver.",
    "Première astuce, fais une liste avant d'aller faire tes courses.",
    "Première astuce : fais toujours une liste avant d'aller faire tes courses.",
    "Deuxième astuce, compare le prix au kilo, pas le prix de la boîte.",
    "Et troisième astuce, attends vingt-quatre heures avant un achat coup de cœur.",
    "Voilà, abonne-toi pour la suite, à bientôt !",
]


def ffmpeg():
    try:
        import imageio_ffmpeg
        return imageio_ffmpeg.get_ffmpeg_exe()
    except ImportError:
        return "ffmpeg"


def voice(tmp):
    """Voix : les phrases, séparées de blancs, en 48 kHz mono."""
    parts = []
    for i, text in enumerate(PHRASES):
        path = os.path.join(tmp, f"p{i}.wav")
        subprocess.run(["espeak-ng", "-v", "fr", "-s", "150", "-p", "55", "-w", path, text], check=True)
        out = os.path.join(tmp, f"p{i}_48.wav")
        subprocess.run([ffmpeg(), "-v", "error", "-y", "-i", path, "-ar", str(SR), "-ac", "1", out], check=True)
        with wave.open(out) as w:
            parts.append(np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32) / 32768)
        parts.append(np.zeros(int(SR * (1.3 if i in (0, 2) else 1.0))))
    return np.concatenate([np.zeros(int(SR * 0.5))] + parts)


def draw(frame_i, mouth, rng_blink):
    t = frame_i / FPS
    img = np.zeros((H, W, 3), np.uint8)
    # Pièce : mur dégradé, étagère, plante, bureau.
    for y in range(H):
        k = y / H
        img[y] = (int(205 - 40 * k), int(214 - 30 * k), int(226 - 20 * k))
    cv2.rectangle(img, (0, 1500), (W, H), (92, 120, 150), -1)
    cv2.rectangle(img, (90, 420), (420, 445), (90, 120, 160), -1)
    for i, c in enumerate([(70, 90, 200), (60, 160, 220), (160, 110, 70)]):
        cv2.rectangle(img, (110 + i * 70, 300), (160 + i * 70, 420), c, -1)
    cv2.ellipse(img, (900, 1380), (90, 140), 0, 0, 360, (60, 140, 80), -1)
    cv2.rectangle(img, (850, 1440), (950, 1560), (70, 90, 160), -1)
    sway = int(12 * np.sin(t * 1.3))
    cx, cy = W // 2 + sway, 760
    # Corps (pull rayé).
    body = np.array([[cx - 330, 1920], [cx - 280, 1260], [cx - 120, 1130], [cx + 120, 1130], [cx + 280, 1260], [cx + 330, 1920]])
    cv2.fillPoly(img, [body], (190, 140, 90))
    mask = np.zeros((H, W), np.uint8)
    cv2.fillPoly(mask, [body], 255)
    stripes = img.copy()
    for y in range(1180, 1920, 60):
        cv2.line(stripes, (cx - 330, y), (cx + 330, y), (225, 225, 235), 14)
    img[mask > 0] = stripes[mask > 0]
    cv2.rectangle(img, (cx - 70, 1020), (cx + 70, 1160), (150, 185, 225), -1)  # cou
    # Tête, cheveux, oreilles.
    cv2.ellipse(img, (cx, cy - 40), (300, 330), 0, 180, 360, (40, 60, 95), -1)
    cv2.ellipse(img, (cx - 250, cy + 40), (60, 90), 0, 0, 360, (40, 60, 95), -1)
    cv2.ellipse(img, (cx + 250, cy + 40), (60, 90), 0, 0, 360, (40, 60, 95), -1)
    cv2.ellipse(img, (cx, cy + 40), (240, 290), 0, 0, 360, (160, 195, 235), -1)
    cv2.ellipse(img, (cx, cy - 170), (250, 140), 0, 180, 360, (40, 60, 95), -1)
    # Yeux (clignements), lunettes, sourcils.
    blink = 0.12 if rng_blink else 1.0
    for dx in (-95, 95):
        cv2.ellipse(img, (cx + dx, cy + 10), (34, int(26 * blink) + 1), 0, 0, 360, (250, 250, 250), -1)
        if blink > 0.5:
            cv2.circle(img, (cx + dx, cy + 12), 15, (60, 40, 30), -1)
        cv2.circle(img, (cx + dx, cy + 10), 70, (40, 40, 40), 8)
        cv2.line(img, (cx + dx - 45, cy - 80), (cx + dx + 45, cy - 86 - int(8 * mouth)), (40, 60, 95), 12)
    cv2.line(img, (cx - 25, cy + 10), (cx + 25, cy + 10), (40, 40, 40), 8)
    cv2.line(img, (cx, cy + 50), (cx - 18, cy + 120), (130, 160, 210), 6)  # nez
    # Bouche : ouverte selon la voix.
    h = int(6 + 46 * mouth)
    cv2.ellipse(img, (cx, cy + 190), (70, h), 0, 0, 360, (60, 50, 150), -1)
    cv2.ellipse(img, (cx, cy + 190), (70, h), 0, 0, 360, (90, 90, 190), 6)
    return img


def main():
    out = sys.argv[1]
    with tempfile.TemporaryDirectory() as tmp:
        audio = voice(tmp)
        wav = os.path.join(tmp, "voix.wav")
        with wave.open(wav, "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(SR)
            w.writeframes((np.clip(audio, -1, 1) * 32767).astype(np.int16).tobytes())
        n = int(len(audio) / SR * FPS)
        hop = SR // FPS
        env = np.array([np.sqrt(np.mean(audio[i * hop:(i + 1) * hop] ** 2)) for i in range(n)])
        env = np.clip(env / (np.percentile(env, 95) + 1e-6), 0, 1)
        rng = np.random.default_rng(3)
        blinks = set()
        f = 0
        while f < n:
            f += int(rng.uniform(60, 140))
            blinks.update({f, f + 1, f + 2})
        proc = subprocess.Popen([ffmpeg(), "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgr24", "-s", f"{W}x{H}",
                                 "-r", str(FPS), "-i", "-", "-i", wav, "-c:v", "libx264", "-preset", "veryfast",
                                 "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "128k", "-shortest", out],
                                stdin=subprocess.PIPE)
        smooth = 0.0
        for i in range(n):
            smooth = 0.5 * smooth + 0.5 * env[i]
            proc.stdin.write(draw(i, smooth, i in blinks).tobytes())
        proc.stdin.close()
        proc.wait()
    print(f"Rush de démonstration : {out} ({n / FPS:.1f} s)")


if __name__ == "__main__":
    main()
