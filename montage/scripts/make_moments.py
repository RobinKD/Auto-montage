"""Prépare la page de sélection des moments (cases à cocher) pour le rush en cours.

Entrées : work/segments.json (segments de parole transcrits, par prepare.sh),
          work/rush.txt, et work/suggestions.json si Claude en a écrit une
          ({"keep": [ids], "reasons": {"id": "raison"}}).
Sorties : work/moments/index.html     page à publier (gabarit selection/page.html)
          work/moments/clips/*.mp4    un extrait vidéo par moment (360x640, avec le son)
          work/moments/thumbs/*.jpg   une vignette par moment
          work/moments/montage.mp4    la version de travail actuelle (out/montage_apercu.mp4,
                                      compressée pour la page), si elle existe
Le bouton « Générer la version de travail » de la page programme dans ~1 min la routine dont
l'id est dans work/regen_trigger.txt (connecteur Claude Code Remote, outil update_trigger).

Sans work/suggestions.json, la pré-sélection est automatique : un segment dont la phrase
est reprise juste après est une 1re prise (retirée, on garde la 2e) ; le reste est gardé.
"""
import datetime
import html
import json
import os
import shutil
import subprocess

import imageio_ffmpeg

from moments_lib import (SOUNDS, VOICES, hf_preview_files, edit_size, frame_layout, res_label, effective_gaps, load_segments, rush_duration, sound_catalog, suggested_keep,
                         wav_len)
import fonts_lib
from progress import report

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
WORK = os.path.join(ROOT, "work")
OUT = os.path.join(WORK, "moments")
FFMPEG = imageio_ffmpeg.get_ffmpeg_exe()

# Moments de parole : transcription, avec les scissions et fusions faites sur la page
# (work/decoupage.json, moments_lib.effective_segments).
raw_segments, segments, _deco = load_segments(WORK)
rush = open(os.path.join(WORK, "rush.txt")).read().strip()


keep, reasons = suggested_keep(segments, WORK)

edit_path = os.path.join(ROOT, "src", "data", "edit.json")
edit_data = json.load(open(edit_path)) if os.path.exists(edit_path) else {}
overlay = edit_data.get("overlayDefaults", {})
face = json.load(open(os.path.join(ROOT, "src", "data", "face.json")))
WIDTH, HEIGHT = edit_size(edit_data)  # format du rush (9:16, 16:9, 4:3…)
# Extraits (petit côté de 360) et version de travail pour la page (720) : 360x640 et 720x1280 en 9:16.
SMALL, WORKING = (f"scale={res_label(WIDTH, HEIGHT, s).replace('x', ':')}" for s in (360, 720))


def face_at(t):
    p = min(face, key=lambda f: abs(f["t"] - t))
    return [p["x"], p["y"]]


for sub in ("clips", "thumbs"):
    os.makedirs(os.path.join(OUT, sub), exist_ok=True)
source = os.path.join(ROOT, "public", "rushes", "rush_1080.mp4")  # H.264, lisible partout
moments = []
# Avancement : un pas par extrait (moments puis plans sans parole), puis la version de travail
# pour la page (deux passes, comptées comme autant d'extraits que le rush a de moments).
gap_list = effective_gaps(raw_segments, segments, rush_duration(WORK))
# Extraits déjà faits pour les mêmes bornes : gardés (une scission ou une fusion ne refait
# que les moments touchés). clips/.bornes.json : nom -> [début, fin].
sig_path = os.path.join(OUT, "clips", ".bornes.json")
try:
    signatures = json.load(open(sig_path))
except (OSError, ValueError):
    signatures = {}


def extract(name, a, b, crf, abr):
    sig = [round(a, 3), round(b, 3)] + ([f"{WIDTH}x{HEIGHT}"] if (WIDTH, HEIGHT) != (1080, 1920) else [])
    clip, thumb = os.path.join(OUT, "clips", name + ".mp4"), os.path.join(OUT, "thumbs", name + ".jpg")
    if signatures.get(name) == sig and os.path.exists(clip) and os.path.exists(thumb):
        return
    subprocess.run(
        [FFMPEG, "-v", "error", "-y", "-ss", f"{a:.3f}", "-to", f"{b:.3f}", "-i", source,
         "-vf", SMALL, "-c:v", "libx264", "-preset", "veryfast", "-crf", str(crf), "-pix_fmt", "yuv420p",
         "-c:a", "aac", "-b:a", abr, "-ac", "1", "-movflags", "+faststart", clip],
        check=True,
    )
    subprocess.run(
        [FFMPEG, "-v", "error", "-y", "-ss", f"{(a + b) / 2:.3f}", "-i", source, "-frames:v", "1",
         "-vf", "scale=120:-2", "-q:v", "5", thumb],
        check=True,
    )
    signatures[name] = sig

preview = os.path.join(ROOT, "out", "montage_apercu.mp4")
steps_total = len(segments) + len(gap_list) + (max(4, len(segments) // 2) if os.path.exists(preview) else 0)
steps_done = 0
report("Découpage des moments", 0, steps_total or 1)
for s in segments:
    name = f"m_{s['id']:03d}"
    a, b = max(0, s["start"] - 0.05), s["end"] + 0.05
    extract(name, a, b, 28, "64k")
    # Mots avec leurs temps dans l'extrait (qui commence à « a »), corrections du montage appliquées.
    fixes = overlay.get("fixes", {}).get(str(s["id"]), {})
    words = [
        {"w": fixes.get(str(i), w["w"]), "a": round(w["a"] - a, 3), "b": round(w["b"] - a, 3)}
        for i, w in enumerate(s["words"])
    ]
    moments.append({
        "id": s["id"],
        "start": round(s["start"], 2),
        "end": round(s["end"], 2),
        "text": " ".join(w["w"] for w in words if w["w"]) or s["text"],
        "words": words,
        "face": face_at((s["start"] + s["end"]) / 2),
        "cashWord": overlay.get("cashWord", {}).get(str(s["id"])),
        "chip": overlay.get("chips", {}).get(str(s["id"])),
        "vfxExtra": overlay.get("vfx", {}).get(str(s["id"]), []),  # effets visuels placés par Claude
        "suggested": s["id"] in keep,
        "reason": reasons.get(s["id"], ""),
        "clip": f"clips/{name}.mp4",
        "thumb": f"thumbs/{name}.jpg",
        # Fusion : ids réunis ; scission : segment d'origine et rang du mot de coupe.
        **({"merged": s["merged"]} if s.get("merged") else {}),
        **({"splitFrom": s["splitFrom"], "splitWord": s["words"][0]["oi"]} if s.get("splitFrom") is not None else {}),
        # Rang de chaque mot dans son segment d'origine (pour scinder ici).
        "origins": [[w["o"], w["oi"]] for w in s["words"]],
    })
    steps_done += 1
    report("Découpage des moments", steps_done, steps_total)

# Plans sans parole (entre les segments) : proposés décochés, à couper à la main sur la page.
for g in gap_list:
    name = f"g_{g['id']:04d}"
    extract(name, g["start"], g["end"], 30, "48k")
    moments.append({
        "id": g["id"], "gap": True, "start": g["start"], "end": g["end"], "text": "", "words": [],
        "face": face_at((g["start"] + g["end"]) / 2), "cashWord": None, "chip": None,
        "suggested": False, "reason": "", "clip": f"clips/{name}.mp4", "thumb": f"thumbs/{name}.jpg",
    })
    steps_done += 1
    report("Découpage des moments", steps_done, steps_total)
moments.sort(key=lambda m: m["start"])
json.dump(signatures, open(sig_path, "w"))

# Version de travail actuelle + correspondance clip du montage -> moment (pour surligner
# le moment en cours pendant la lecture).
working = None
if os.path.exists(preview):
    target = os.path.join(OUT, "montage.mp4")
    # Recompressée seulement si la version de travail a changé depuis (scission, fusion…).
    if not os.path.exists(target) or os.path.getmtime(target) < os.path.getmtime(preview):
        common = [FFMPEG, "-v", "error", "-y", "-i", preview, "-vf", WORKING, "-c:v", "libx264",
                  "-preset", "slow", "-b:v", "2500k", "-pix_fmt", "yuv420p"]
        subprocess.run(common + ["-pass", "1", "-passlogfile", os.path.join(OUT, "pass"), "-an", "-f", "mp4", os.devnull],
                       check=True)
        report("Découpage des moments", (steps_done + steps_total) / 2, steps_total, every=0)
        subprocess.run(common + ["-pass", "2", "-passlogfile", os.path.join(OUT, "pass"), "-c:a", "aac", "-b:a", "128k",
                                 "-movflags", "+faststart", target], check=True)
        for f in os.listdir(OUT):
            if f.startswith("pass"):
                os.remove(os.path.join(OUT, f))
    report("Découpage des moments", steps_total, steps_total, every=0)
    edit = json.load(open(os.path.join(ROOT, "src", "data", "edit.json")))
    fps = edit["fps"]
    working = {
        "src": "montage.mp4",
        "duration": round(edit["durationInFrames"] / fps, 2),
        # Heure du rendu (UTC), affichée sur la page pour voir que la version a changé.
        "renderedAt": datetime.datetime.fromtimestamp(os.path.getmtime(preview), datetime.timezone.utc).isoformat(),
        "clips": [{"seg": c["seg"], "start": round(c["from"] / fps, 3),
                   "end": round((c["from"] + c["durationInFrames"]) / fps, 3)} for c in edit["clips"]],
    }

# Effets sonores proposés par moment (les défauts du montage, ex. la caisse sur l'argent).
defaults = edit_data.get("sfxDefaults", {})
for m in moments:
    m["sfx"] = defaults.get(str(m["id"]), [])
# Sons et police pour l'habillage des moments dans la page.
os.makedirs(os.path.join(OUT, "sfx"), exist_ok=True)
# Sons personnels (public/sfx/perso/<nom>.wav, envoyés depuis la page « Rushes et montages ») :
# effets « perso-<nom> » en plus de ceux du projet.
perso_dir = os.path.join(ROOT, "public", "sfx", "perso")
perso = sorted(n[:-4] for n in os.listdir(perso_dir) if n.endswith(".wav")) if os.path.isdir(perso_dir) else []
sources = [(n, os.path.join(ROOT, "public", "sfx", f"{n}.wav")) for n in ("key-1", "key-2", "key-3", "key-4")]
sources += [(i, os.path.join(ROOT, "public", "sfx", f)) for i, _, f, _ in SOUNDS if f]
sources += [(f"perso-{n}", os.path.join(perso_dir, f"{n}.wav")) for n in perso]
for name, src in sources:
    subprocess.run([FFMPEG, "-v", "error", "-y", "-i", src,
                    "-c:a", "libmp3lame", "-b:a", "96k", os.path.join(OUT, "sfx", f"{name}.mp3")], check=True)
oliver = [f for f in os.listdir(os.path.join(ROOT, "public", "fonts")) if f.lower().startswith("oliver")]
font = None
if oliver:
    os.makedirs(os.path.join(OUT, "fonts"), exist_ok=True)
    font = "fonts/oliver" + os.path.splitext(oliver[0])[1].lower()
    shutil.copy(os.path.join(ROOT, "public", "fonts", oliver[0]), os.path.join(OUT, font))

# Sons par défaut (moments_lib.SOUNDS), voix modifiée (voix qui tremble), puis sons personnels.
sfx_catalog = sound_catalog() + [{"id": f"perso-{n}", "label": n.replace("-", " ").capitalize(), "vol": 0.6,
                                  "len": wav_len(os.path.join(perso_dir, f"{n}.wav"))} for n in perso]
trigger_path = os.path.join(WORK, "regen_trigger.txt")
trigger = open(trigger_path).read().strip() if os.path.exists(trigger_path) else None
# Page « Téléchargements » (scripts/make_downloads.py), lien affiché sous le lecteur.
downloads_path = os.path.join(WORK, "downloads_url.txt")
downloads_page = open(downloads_path).read().strip() if os.path.exists(downloads_path) else None

# Nombre de lignes des sous-titres et taille de chacune (page publiée : sans le style de l'interface locale).
caption_lines = fonts_lib.read_style()
hf_preview_files(OUT)  # aperçu des voix faites par HyperFrames (hf/)
page = open(os.path.join(ROOT, "selection", "page.html")).read()
data = {"rush": os.path.basename(rush), "width": WIDTH, "height": HEIGHT, "layout": frame_layout(WIDTH, HEIGHT), "moments": moments, "working": working,
        "sfxCatalog": sfx_catalog, "regenTrigger": trigger, "downloadsPage": downloads_page,
        "intro": {"text": overlay.get("intro"), "at": overlay.get("introAt", 0.2),
                  "type": overlay.get("introType", 1.6)} if overlay.get("intro") else None,
        "captionFont": font, "captionLines": {k: caption_lines[k] for k in ("lines", "lineSizes")},
        "voices": [{"id": i, "label": label, "desc": desc} for i, label, desc in VOICES]}
# Nom du rush échappé, données sans « < » (une transcription contenant « </script> » ne
# referme pas le script de la page).
page = page.replace("__RUSH__", html.escape(os.path.splitext(os.path.basename(rush))[0]))
page = page.replace("/*__DATA__*/null", json.dumps(data, ensure_ascii=False).replace("<", "\\u003c"))
open(os.path.join(OUT, "index.html"), "w").write(page)
kept = sum(m["end"] - m["start"] for m in moments if m["suggested"])
n_gaps = sum(1 for m in moments if m.get("gap"))
print(f"{len(moments) - n_gaps} moments de parole + {n_gaps} plans sans parole, "
      f"{sum(m['suggested'] for m in moments)} pré-cochés ({kept:.1f} s) -> work/moments/")
