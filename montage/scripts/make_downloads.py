"""Prépare la page « Téléchargements » : la version de travail en 720p, 1080p et 4K.

Sources (celles qui existent) :
  720p   work/moments/montage.mp4   (copie légère de la page des moments, make_moments.py)
  1080p  out/montage_apercu.mp4     (./scripts/render.sh apercu)
  4K     out/montage_4k.mp4         (./scripts/render.sh final)
Un fichier de moins de 15 Mo est publié tel quel ; au-delà, il est découpé en morceaux de
14 Mo que la page rassemble en un seul MP4 au téléchargement (limite de 15 Mo par fichier
publié). Une page tient au plus 256 Mo : si l'ensemble dépasse BUDGET, la copie 4K de la
page est réencodée (2 passes) juste assez pour tenir ; le rendu 4K d'origine n'est pas touché.

Sorties : work/downloads/index.html   page à publier (gabarit selection/downloads.html)
          work/downloads/files/*      fichiers et morceaux, noms datés du rendu
          work/downloads/publish.json lots de fichiers à publier (64 Mo max par publication)
                                      et fichiers de l'ancienne version à retirer (null)
"""
import datetime
import html
import json
import os
import subprocess

import imageio_ffmpeg

from moments_lib import edit_size, res_label

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
WORK = os.path.join(ROOT, "work")
OUT = os.path.join(WORK, "downloads")
FILES = os.path.join(OUT, "files")
FFMPEG = imageio_ffmpeg.get_ffmpeg_exe()
MB = 1024 * 1024
SINGLE_MAX = 15 * MB - 64 * 1024  # un fichier publié : 15 Mo max
PART = 14 * MB
BUDGET = 245 * MB  # page entière (256 Mo max), marge pour la page elle-même
BATCH = 60 * MB  # une publication : 64 Mo max

rush = os.path.splitext(os.path.basename(open(os.path.join(WORK, "rush.txt")).read().strip()))[0]
try:  # format du rush (9:16, 16:9, 4:3…)
    SIZE = edit_size(json.load(open(os.path.join(ROOT, "src", "data", "edit.json"))))
except (OSError, ValueError):
    SIZE = edit_size(None)


def res(w, h):  # « 720x1280 » en 9:16 : petit côté de w, au format du rush
    return res_label(*SIZE, w)


SOURCES = [
    ("720p", "Légère", res(720, 1280), os.path.join(WORK, "moments", "montage.mp4"),
     "Copie compressée de la version de travail, pour la partager ou la regarder sur téléphone."),
    ("1080p", "1080p", res(1080, 1920), os.path.join(ROOT, "out", "montage_apercu.mp4"),
     "Rendu de l'aperçu, à la résolution de la composition."),
    ("4k", "4K", res(2160, 3840), os.path.join(ROOT, "out", "montage_4k.mp4"),
     "Rendu final (./scripts/render.sh final)."),
]


def duration(path):
    out = subprocess.run([FFMPEG, "-i", path], capture_output=True, text=True).stderr
    h, m, s = out.split("Duration: ")[1].split(",")[0].split(":")
    return int(h) * 3600 + int(m) * 60 + float(s)


def stamp_of(path):
    return datetime.datetime.fromtimestamp(os.path.getmtime(path), datetime.timezone.utc)


def fit(src, target_bytes):
    """Copie réencodée (2 passes) de src qui pèse environ target_bytes, en cache."""
    dst = os.path.join(OUT, "fit_" + os.path.basename(src))
    key = f"{os.path.getmtime(src)}:{target_bytes}"
    key_path = dst + ".key"
    if os.path.exists(dst) and os.path.exists(key_path) and open(key_path).read() == key:
        return dst
    audio = 192_000
    video = int(target_bytes * 8 * 0.97 / duration(src)) - audio
    log = os.path.join(OUT, "fit_pass")
    common = [FFMPEG, "-v", "error", "-y", "-i", src, "-c:v", "libx264", "-preset", "medium",
              "-b:v", str(video), "-pix_fmt", "yuv420p", "-passlogfile", log]
    print(f"Réencodage de {os.path.basename(src)} à {video / 1e6:.1f} Mb/s pour tenir sur la page…")
    subprocess.run(common + ["-pass", "1", "-an", "-f", "mp4", os.devnull], check=True)
    subprocess.run(common + ["-pass", "2", "-c:a", "aac", "-b:a", str(audio), "-movflags", "+faststart", dst],
                   check=True)
    for f in os.listdir(OUT):
        if f.startswith("fit_pass"):
            os.remove(os.path.join(OUT, f))
    open(key_path, "w").write(key)
    return dst


os.makedirs(FILES, exist_ok=True)
available = [(vid, label, res, path, note) for vid, label, res, path, note in SOURCES if os.path.exists(path)]
sizes = {vid: os.path.getsize(path) for vid, _, _, path, _ in available}
# Trop lourd pour une page : la plus grosse copie (la 4K) est réencodée pour tenir.
fitted = {}
if sum(sizes.values()) > BUDGET:
    big = max(sizes, key=sizes.get)
    target = BUDGET - (sum(sizes.values()) - sizes[big])
    src = dict((v, p) for v, _, _, p, _ in available)[big]
    fitted[big] = (fit(src, target), sizes[big])

previous = set()
publish_path = os.path.join(OUT, "publish.json")
if os.path.exists(publish_path):
    previous = set(json.load(open(publish_path)).get("published", []))

apercu = os.path.join(ROOT, "out", "montage_apercu.mp4")
versions, current = [], []
for f in os.listdir(FILES):
    os.remove(os.path.join(FILES, f))
for vid, label, res, path, note in available:
    rendered = stamp_of(path)
    tag = rendered.strftime("%Y%m%d-%H%M%S")
    src = fitted.get(vid, (path, None))[0]
    size = os.path.getsize(src)
    data = open(src, "rb").read()
    base = f"{rush}_{vid}_{tag}"
    if size <= SINGLE_MAX:
        names = [f"files/{base}.mp4"]
        open(os.path.join(OUT, names[0]), "wb").write(data)
    else:  # morceaux bruts (en .mp4 : type servi par les pages), rassemblés dans l'ordre par la page
        names = []
        for k in range(0, size, PART):
            name = f"files/{base}.part{k // PART + 1:02d}.mp4"
            open(os.path.join(OUT, name), "wb").write(data[k:k + PART])
            names.append(name)
    current += names
    versions.append({
        "id": vid, "label": label, "res": res, "note": note,
        "filename": f"{rush}_{vid}_{rendered.strftime('%Y-%m-%d_%Hh%M')}.mp4",
        "size": size, "duration": round(duration(src), 1), "renderedAt": rendered.isoformat(),
        "parts": names,
        "reencoded": round(sizes[vid] / MB) if vid in fitted else None,
        # Rendu plus ancien que la version de travail actuelle (ex. 4K d'un montage précédent).
        "stale": vid != "720p" and os.path.exists(apercu) and os.path.getmtime(path) < os.path.getmtime(apercu) - 60,
    })

moments_url = None
url_path = os.path.join(WORK, "moments_url.txt")
if os.path.exists(url_path):
    moments_url = open(url_path).read().strip()
page = open(os.path.join(ROOT, "selection", "downloads.html")).read()
page = page.replace("__RUSH__", html.escape(rush))
page = page.replace("/*__DATA__*/null", json.dumps({"rush": rush, "versions": versions, "momentsPage": moments_url},
                                                   ensure_ascii=False).replace("<", "\\u003c"))
open(os.path.join(OUT, "index.html"), "w").write(page)

# Lots de publication : 64 Mo max par publication, les anciens fichiers à retirer.
batches, cur, cur_size = [], [], 0
for name in current:
    s = os.path.getsize(os.path.join(OUT, name))
    if cur and cur_size + s > BATCH:
        batches.append(cur)
        cur, cur_size = [], 0
    cur.append(name)
    cur_size += s
if cur:
    batches.append(cur)
json.dump({"batches": batches, "remove": sorted(previous - set(current)), "published": current},
          open(publish_path, "w"), indent=1)
for v in versions:
    extra = f", réencodée depuis {v['reencoded']} Mo" if v["reencoded"] else ""
    print(f"{v['id']:>5} : {v['size'] / MB:6.1f} Mo en {len(v['parts'])} fichier(s){extra}"
          f"{' (ancienne)' if v['stale'] else ''}")
print(f"{len(batches)} publication(s), {len(previous - set(current))} ancien(s) fichier(s) à retirer -> work/downloads/")
