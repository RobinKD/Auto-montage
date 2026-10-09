"""Montages enregistrés : met de côté le montage du rush en cours et le rouvre plus tard.

Un montage, c'est tout ce qui a été calculé ou choisi pour un rush : versions de travail
(1080p, source 4K), transcription, page des moments, choix de la page (variantes, corrections,
effets, découpes), choix de montage, rendus et discussion avec Claude. Le mettre de côté le
range dans work/projets/<rush>/ ; le rouvrir le remet en place sans rien recalculer.

Usage : python3 scripts/project.py save            met de côté le montage en cours
        python3 scripts/project.py open <dossier>   rouvre un montage (l'actuel est mis de côté)
        python3 scripts/project.py delete <dossier> supprime un montage enregistré
        python3 scripts/project.py list             montages enregistrés (JSON)
prepare.sh appelle « save » quand le rush change ; la page « Nouveau rush » (interface locale)
appelle les autres.
"""
import datetime
import json
import os
import re
import shutil
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
WORK = os.path.join(ROOT, "work")
STORE = os.path.join(WORK, "projets")
# Fichiers de work/ communs à tous les rushes : jamais déplacés.
# Les consignes (work/instructions.*) et la langue (langue.txt) sont choisies avant de préparer le
# rush suivant : elles restent (langue_rush.txt, la langue du rush, est rangée avec lui).
SHARED = {"projets", "local_db", "timings.json", "models", "regen_trigger.txt", "instructions.md", "instructions.name", "langue.txt", "mode.txt", "derush.json",
          "style", "logs", "sauvegardes", "caption_style_default.json", "effets_page.json",
          "tutoriel_vu", "rendu_hyperframes", "rendu_parties", "memoire_mesures.json",
          "tache.json", "reprise"}  # préparation en cours ou en pause (local_server.py) : jamais rangée
# Hors de work/ : déplacés (lourds) ou copiés (suivis par git, restent en place).
MOVED = ["public/rushes/rush_1080.mp4", "public/rushes/rush_1080.webm", "public/rushes/rush_2160.webm",
         "public/audio", "out/montage_apercu.mp4", "out/montage_4k.mp4"]
COPIED = ["src/data/edit.json", "src/data/face.json"]


def read(path):
    try:
        return open(path, encoding="utf-8").read().strip()
    except OSError:
        return None


def size_of(path):
    if os.path.isfile(path):
        return os.path.getsize(path)
    return sum(os.path.getsize(os.path.join(d, f)) for d, _, files in os.walk(path) for f in files)


def move(src, dst):
    if os.path.lexists(src):
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        if os.path.isdir(dst) and not os.path.islink(dst):
            shutil.rmtree(dst)
        elif os.path.lexists(dst):
            os.remove(dst)
        shutil.move(src, dst)


def page_docs(rush):
    """Choix de la page des moments (work/local_db) qui visent ce rush : (collection, fichier)."""
    db = os.path.join(WORK, "local_db")
    out = []
    for coll in sorted(os.listdir(db)) if os.path.isdir(db) else []:
        folder = os.path.join(db, coll)
        for name in sorted(os.listdir(folder)) if os.path.isdir(folder) else []:
            try:
                if name.endswith(".json") and json.load(open(os.path.join(folder, name))).get("rush") == rush:
                    out.append((coll, name))
            except (OSError, ValueError, AttributeError):
                pass
    return out


def summary():
    """Description du montage en cours (pour la liste des montages enregistrés)."""
    info = {"rush": os.path.basename(read(os.path.join(WORK, "rush.txt")) or ""),
            "key": read(os.path.join(WORK, "rush_key.txt")),
            "savedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")}
    try:
        edit = json.load(open(os.path.join(ROOT, "src", "data", "edit.json")))
        info["duration"] = round(edit["durationInFrames"] / edit["fps"], 1)
    except (OSError, ValueError, KeyError, ZeroDivisionError):
        info["duration"] = None
    info["has4k"] = os.path.exists(os.path.join(ROOT, "out", "montage_4k.mp4"))
    info["hasPage"] = os.path.exists(os.path.join(WORK, "moments", "index.html"))
    return info


def folder_name(rush):
    stem = re.sub(r"[^A-Za-z0-9_.-]+", "_", os.path.splitext(rush)[0]) or "rush"
    return stem


def save():
    """Met de côté le montage en cours ; ne fait rien s'il n'y en a pas."""
    rush_path = read(os.path.join(WORK, "rush.txt"))
    if not rush_path:
        return None
    info = summary()
    name = folder_name(info["rush"])
    dest = os.path.join(STORE, name)
    if os.path.isdir(dest):
        old = read(os.path.join(dest, "projet.json"))
        old_key = json.loads(old).get("key") if old else None
        if old_key and info["key"] and old_key != info["key"]:
            # Autre vidéo sous le même nom : l'ancien montage garde sa place, sous un autre nom.
            os.rename(dest, f"{dest}-{datetime.datetime.now().strftime('%Y%m%d-%H%M%S')}")
        else:
            shutil.rmtree(dest)  # même rush, enregistré plus tôt : remplacé par l'état actuel
    os.makedirs(dest)
    for entry in os.listdir(WORK):
        if entry not in SHARED:
            move(os.path.join(WORK, entry), os.path.join(dest, "work", entry))
    for rel in MOVED:
        move(os.path.join(ROOT, rel), os.path.join(dest, rel))
    # Vidéos du dernier rendu (liens ou copies de rush_1080/rush_2160) : elles garderaient sur le
    # disque les vidéos de ce montage, même supprimé ; refaites au prochain rendu.
    assets = os.path.join(WORK, "rendu_hyperframes", "assets")
    for entry in os.listdir(assets) if os.path.isdir(assets) else []:
        if entry.lower().endswith((".mp4", ".webm", ".mov")):
            os.remove(os.path.join(assets, entry))
    for rel in COPIED:
        if os.path.exists(os.path.join(ROOT, rel)):
            os.makedirs(os.path.dirname(os.path.join(dest, rel)), exist_ok=True)
            shutil.copy2(os.path.join(ROOT, rel), os.path.join(dest, rel))
    for coll, fname in page_docs(info["rush"]):
        move(os.path.join(WORK, "local_db", coll, fname), os.path.join(dest, "local_db", coll, fname))
    info["size"] = size_of(dest)
    json.dump(info, open(os.path.join(dest, "projet.json"), "w"), ensure_ascii=False, indent=1)
    print(f"Montage de {info['rush']} mis de côté dans work/projets/{name}")
    return name


def open_project(name):
    src = os.path.join(STORE, name)
    if not name or "/" in name or name.startswith(".") or not os.path.isfile(os.path.join(src, "projet.json")):
        sys.exit(f"Montage enregistré introuvable : {name}")
    info = json.load(open(os.path.join(src, "projet.json")))
    if read(os.path.join(WORK, "rush.txt")):
        if os.path.basename(read(os.path.join(WORK, "rush.txt"))) == info["rush"] and \
                read(os.path.join(WORK, "rush_key.txt")) == info.get("key"):
            # Même rush préparé à nouveau entre-temps : l'état actuel remplace l'ancien.
            shutil.rmtree(src)
            print(f"{info['rush']} est déjà le rush en cours")
            return
        # Mis à l'écart le temps d'enregistrer le montage en cours (qui peut prendre ce nom).
        aside = os.path.join(STORE, f".ouverture-{name}")
        os.rename(src, aside)
        src = aside
        save()
    for entry in os.listdir(os.path.join(src, "work")) if os.path.isdir(os.path.join(src, "work")) else []:
        move(os.path.join(src, "work", entry), os.path.join(WORK, entry))
    for rel in MOVED:
        target = os.path.join(ROOT, rel)
        if os.path.lexists(os.path.join(src, rel)):
            move(os.path.join(src, rel), target)
        elif os.path.isdir(target):
            shutil.rmtree(target)
        elif os.path.lexists(target):
            os.remove(target)
    for rel in COPIED:
        if os.path.exists(os.path.join(src, rel)):
            shutil.copy2(os.path.join(src, rel), os.path.join(ROOT, rel))
    db = os.path.join(src, "local_db")
    for coll in os.listdir(db) if os.path.isdir(db) else []:
        for fname in os.listdir(os.path.join(db, coll)):
            move(os.path.join(db, coll, fname), os.path.join(WORK, "local_db", coll, fname))
    shutil.rmtree(src)
    missing = not os.path.exists(os.path.join(ROOT, read(os.path.join(WORK, "rush.txt")) or "-"))
    print(f"Montage de {info['rush']} rouvert" + (" (vidéo d'origine absente de public/rushes : "
                                                    "la 4K ne pourra pas être refaite)" if missing else ""))


def delete(name):
    path = os.path.join(STORE, name)
    if not name or "/" in name or name.startswith(".") or not os.path.isfile(os.path.join(path, "projet.json")):
        sys.exit(f"Montage enregistré introuvable : {name}")
    shutil.rmtree(path)
    print(f"Montage {name} supprimé")


def listing():
    out = []
    for name in sorted(os.listdir(STORE)) if os.path.isdir(STORE) else []:
        meta = os.path.join(STORE, name, "projet.json")
        try:
            info = json.load(open(meta))
        except (OSError, ValueError):
            continue
        rush_file = os.path.join(ROOT, "public", "rushes", info.get("rush", ""))
        thumbs = os.path.join(STORE, name, "work", "moments", "thumbs")
        has_thumb = os.path.isdir(thumbs) and any(n.startswith("m_") for n in os.listdir(thumbs))
        out.append(info | {"id": name, "rushPresent": os.path.isfile(rush_file),
                           "thumb": f"/api/projects/{name}/thumb" if has_thumb else None})
    return sorted(out, key=lambda p: p.get("savedAt", ""), reverse=True)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "save":
        save()
    elif cmd == "open" and len(sys.argv) == 3:
        open_project(sys.argv[2])
    elif cmd == "delete" and len(sys.argv) == 3:
        delete(sys.argv[2])
    elif cmd == "list":
        print(json.dumps(listing(), ensure_ascii=False, indent=1))
    else:
        sys.exit(__doc__)
