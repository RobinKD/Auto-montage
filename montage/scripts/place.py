"""Place prise sur le disque par Auto-montage, et ce qu'on peut supprimer pour en libérer
(page « Rushes et montages », section « Libérer de la place »).

Éléments proposés :
  projet:<dossier>     montage enregistré (work/projets/<dossier>)
  rush:<fichier>       vidéo envoyée (public/rushes), copie de l'original de l'ordinateur
  hd                   source 4K du montage en cours (public/rushes/rush_2160.webm), refaite à la
                       prochaine création 4K
  rendu                copies des vidéos laissées par le dernier rendu (work/rendu_hyperframes/assets),
                       refaites au prochain rendu
  parties              parties d'un long rendu interrompu (work/rendu_parties), refaites si effacées
  sauvegarde:<dossier> fichiers remplacés par une réinitialisation (work/sauvegardes)
  restes               envois coupés (.envoi-*), fichiers d'une tâche arrêtée en route (*.part.*) et
                       images extraites par HyperFrames restées dans le dossier temporaire

Usage : python3 scripts/place.py                  liste (JSON)
        python3 scripts/place.py delete <id>…     supprime (seulement des éléments de la liste)
Le serveur ne l'appelle que quand aucune tâche ne tourne (« restes » serait sinon une tâche en cours).
"""
import datetime
import glob
import json
import os
import re
import shutil
import sys
import tempfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
WORK = os.path.join(ROOT, "work")
STORE = os.path.join(WORK, "projets")
RUSHES = os.path.join(ROOT, "public", "rushes")
OUT = os.path.join(ROOT, "out")
ASSETS = os.path.join(WORK, "rendu_hyperframes", "assets")
PARTS = os.path.join(WORK, "rendu_parties")  # parties d'un long rendu interrompu
BACKUPS = os.path.join(WORK, "sauvegardes")
VIDEO_EXT = (".mov", ".mp4", ".m4v", ".mkv", ".webm", ".avi", ".mts")
DERIVED = re.compile(r"^rush_(1080|2160)\.")  # versions de travail du montage en cours


def read(path):
    try:
        return open(path, encoding="utf-8").read().strip()
    except OSError:
        return None


def freed(path):
    """Place rendue par la suppression de path : les fichiers qui ont un autre nom ailleurs (lien
    physique, comme les vidéos du rendu) ne libèrent rien tant que l'autre nom reste."""
    if os.path.islink(path):
        return 0
    if os.path.isfile(path):
        st = os.stat(path)
        return st.st_size if st.st_nlink == 1 else 0
    total = 0
    for folder, _, names in os.walk(path):
        for name in names:
            try:
                st = os.lstat(os.path.join(folder, name))
            except OSError:
                continue
            if st.st_nlink == 1 and not os.path.islink(os.path.join(folder, name)):
                total += st.st_size
    return total


def extract_caches():
    """Images extraites par HyperFrames gardées dans le dossier temporaire (rendus d'avant la
    0.59, ou lancés à la main) : jusqu'à plus de 10 Go."""
    return glob.glob(os.path.join(tempfile.gettempdir(), "hyperframes-extract-cache-*"))


def leftovers():
    return [os.path.join(d, n) for d in (RUSHES, OUT) if os.path.isdir(d)
            for n in os.listdir(d) if n.startswith(".envoi-") or ".part." in n] + extract_caches()


def render_copies():
    return [os.path.join(ASSETS, n) for n in os.listdir(ASSETS)
            if n.lower().endswith(VIDEO_EXT)] if os.path.isdir(ASSETS) else []


def render_parts():
    return [os.path.join(PARTS, n) for n in os.listdir(PARTS)] if os.path.isdir(PARTS) else []


def day(ts):
    return datetime.datetime.fromtimestamp(ts).strftime("%d/%m/%Y")


def inventory():
    current = os.path.basename(read(os.path.join(WORK, "rush.txt")) or "")
    projects = {}
    for name in sorted(os.listdir(STORE)) if os.path.isdir(STORE) else []:
        try:
            projects[name] = json.load(open(os.path.join(STORE, name, "projet.json")))
        except (OSError, ValueError):
            continue
    items = []
    for name, info in projects.items():
        rush = info.get("rush") or name
        try:
            saved = "mis de côté le " + day(datetime.datetime.fromisoformat(info["savedAt"]).timestamp())
        except (KeyError, TypeError, ValueError):
            saved = ""
        items.append({"id": f"projet:{name}", "group": "montages", "label": rush, "size": freed(os.path.join(STORE, name)),
                      "detail": saved,
                      "note": "Choix, versions de travail et rendus effacés ; la vidéo envoyée reste (ci-dessous)."})
    used = {}
    for name, info in projects.items():
        used.setdefault(info.get("rush"), []).append(name)
    for name in sorted(os.listdir(RUSHES), key=str.lower) if os.path.isdir(RUSHES) else []:
        path = os.path.join(RUSHES, name)
        if not os.path.isfile(path) or not name.lower().endswith(VIDEO_EXT) or DERIVED.match(name) \
                or name.startswith(".") or ".part." in name:
            continue
        if name == current:
            note = ("Vidéo du montage en cours : sans elle, sa 4K ne pourra plus être créée"
                    + (" (la source 4K est déjà prête, elle suffit tant qu'elle reste)."
                       if os.path.exists(os.path.join(RUSHES, "rush_2160.webm")) else ".")
                    + " Le montage lui-même reste.")
        elif name in used:
            note = "Utilisée par un montage enregistré : il se rouvrira, mais sa 4K ne pourra plus être refaite."
        else:
            note = "Aucun montage ne l'utilise."
        items.append({"id": f"rush:{name}", "group": "videos", "label": name, "size": freed(path),
                      "detail": f"fichier du {day(os.path.getmtime(path))}", "note": note,
                      "keep": name == current or name in used})
    hd = os.path.join(RUSHES, "rush_2160.webm")
    if os.path.isfile(hd):
        items.append({"id": "hd", "group": "videos", "label": "Source 4K du montage en cours", "size": freed(hd),
                      "detail": "rush_2160.webm",
                      "note": "Refaite (plusieurs minutes) à la prochaine création 4K ; la 4K déjà créée reste.",
                      "keep": True})
    copies = render_copies()
    if copies:
        items.append({"id": "rendu", "group": "videos", "label": "Copies des vidéos du dernier rendu",
                      "size": sum(freed(p) for p in copies), "detail": ", ".join(os.path.basename(p) for p in copies),
                      "note": "Refaites au prochain rendu. Elles peuvent garder sur le disque une vidéo déjà supprimée ailleurs."})
    parts = render_parts()
    if parts:
        items.append({"id": "parties", "group": "videos", "label": "Parties d'un rendu interrompu",
                      "size": sum(freed(p) for p in parts), "detail": f"{len(parts)} partie(s)",
                      "note": "Gardées pour reprendre le rendu sans les refaire ; refaites si elles sont effacées."})
    for name in sorted(os.listdir(BACKUPS)) if os.path.isdir(BACKUPS) else []:
        items.append({"id": f"sauvegarde:{name}", "group": "autres", "label": f"Sauvegarde {name}",
                      "size": freed(os.path.join(BACKUPS, name)),
                      "note": "Fichiers remplacés lors d'une réinitialisation d'Auto-montage."})
    rest = leftovers()
    if rest:
        items.append({"id": "restes", "group": "autres", "label": "Envois coupés et fichiers inachevés",
                      "size": sum(freed(p) for p in rest), "detail": ", ".join(os.path.basename(p) for p in rest),
                      "note": "Restes d'un envoi ou d'une tâche interrompus : inutilisables."})
    free = shutil.disk_usage(ROOT).free
    return {"items": items, "free": free}


def remove(path):
    if os.path.isdir(path) and not os.path.islink(path):
        shutil.rmtree(path)
    elif os.path.lexists(path):
        os.remove(path)


def delete(ids):
    known = {i["id"] for i in inventory()["items"]}
    done = []
    for item in ids:
        if item not in known:
            continue
        kind, _, name = item.partition(":")
        paths = {"projet": lambda: [os.path.join(STORE, name)], "rush": lambda: [os.path.join(RUSHES, name)],
                 "sauvegarde": lambda: [os.path.join(BACKUPS, name)], "hd": lambda: [os.path.join(RUSHES, "rush_2160.webm")],
                 "rendu": render_copies, "parties": render_parts, "restes": leftovers}[kind]()
        for p in paths:
            remove(p)
        done.append(item)
    return done


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "delete":
        before = shutil.disk_usage(ROOT).free
        done = delete(sys.argv[2:])
        print(json.dumps({"deleted": done, "freed": max(0, shutil.disk_usage(ROOT).free - before)}))
    else:
        print(json.dumps(inventory(), ensure_ascii=False))
