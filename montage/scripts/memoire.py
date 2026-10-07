"""Mémoire nécessaire à chaque étape, d'après la vidéo, comparée à la mémoire disponible.

Chaque opération a un besoin estimé (pic de mémoire de tous ses programmes) qui dépend de la
taille de l'image, de la durée du rush ou du montage et du nombre de navigateurs du rendu.
Les étapes se suivent : le besoin d'une tâche est celui de son opération la plus gourmande.
Mesures (6 octobre 2026, 4 cœurs, pic de la somme des PSS de tous les processus) : version de
travail 185 à 410 Mo (1080p, 4K vers 1080p, 9:16), transcription 2,1 Go (Whisper large-v3-turbo,
modèle de 1,6 Go), visage 120 Mo, montage (build_edit.py) 150 Mo pour 2 min de rush (son du rush
entier en mémoire). Rendu HyperFrames 1080p (7 octobre, programmes lancés compris, navigateurs
dans leur propre session) avec 4 navigateurs : 4,6 Go pour 80 s de montage en 8 plans, 6 Go pour
5 min en 8 plans, 7 Go pour 5 min en 150 plans (navigateurs de 1,05 à 1,3 Go chacun). Rendu 4K : 4 navigateurs ont dépassé 14 Go (d'où 2 au plus). Le
rendu prend autant de navigateurs que la mémoire libre le permet (choose_workers).

Expérience de la machine : l'interface locale mesure le vrai pic de chaque opération pendant les
préparations (somme des PSS des programmes de l'étape) et le garde dans work/memoire_mesures.json
avec le besoin prévu pour ce rush-là (record). Le besoin prévu d'un nouveau rush est celui de la
formule (qui suit la taille de l'image et la durée), multiplié par le plus grand rapport mesuré /
prévu des 5 dernières mesures de l'opération, plus 10 % de marge (entre 0,5 et 3 fois la formule).

Mémoire disponible : MemAvailable de /proc/meminfo, bornée par la limite du conteneur (cgroup)
quand il y en a une. Avertissement « juste » sous 1,25 fois le besoin, « insuffisant » en dessous.

Usage : python3 scripts/memoire.py <rush> [<vidéo suivante>…]   bilan de la préparation (texte)
prepare.sh l'appelle au début ; le serveur de l'interface locale (local_server.py) s'en sert
avant de préparer et au début de chaque étape.
"""
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from moments_lib import display_size, work_size  # noqa: E402

MO = 2**20
MARGIN = 1.25  # « juste » : moins de 25 % au-dessus du besoin
MEASURES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "work", "memoire_mesures.json")
KEEP_MEASURES = 10  # mesures gardées par opération


def _mpx(size):
    return size[0] * size[1] / 1e6 if size else 2.07


def render_workers(hd=False):
    """Navigateurs du rendu HyperFrames (comme render_hyperframes.py)."""
    return max(1, min(2 if hd else 4, os.cpu_count() or 1))


# Besoin de chaque opération, en Mo : fonction de la vidéo (v : durée « duration » du rush en
# secondes, « source » taille de l'image d'origine, « work » taille de la version de travail,
# « montage » durée du montage en secondes).
OPERATIONS = {
    "Assemblage des vidéos": lambda v: 120,
    "Assemblage des vidéos (réencodage)": lambda v: 150 + 140 * _mpx(v["source"]),
    # Décodage de l'image d'origine et encodage x264 (images d'avance) de la version de travail.
    "Version de travail (MP4)": lambda v: 150 + 124 * _mpx(v["work"]) + 14 * _mpx(v["source"]),
    "Extraction du son": lambda v: 60,
    "Détection de la parole": lambda v: 60,
    # Modèle large-v3-turbo (1,6 Go) et ses calculs ; le son 16 kHz du rush entier en mémoire.
    "Transcription": lambda v: 2300 + 0.1 * v["duration"],
    "Position du visage": lambda v: 100 + 10 * _mpx(v["work"]),
    # Son du rush entier en 48 kHz (entiers puis flottants), lu pour recaler les coupes.
    "Bruitages et montage": lambda v: 250 + 0.3 * v["duration"],
    "Consignes de montage (Claude)": lambda v: 600,
    "Montage selon les consignes": lambda v: 250 + 0.3 * v["duration"],
    # Rendu : HyperFrames (node, encodeur ffmpeg, extractions d'images des plans 4 à la fois), un
    # navigateur par « worker » (plus lourd avec beaucoup de plans : un tous les 2 s de montage
    # environ), et ce qui grandit avec le montage rendu (2,3 Mo par seconde, tous navigateurs).
    "Rendu de la vidéo": lambda v: 1100 + v.get("workers", render_workers()) * (
        450 + 280 * _mpx(v["work"]) + 0.85 * v["montage"]) + 2.3 * v["montage"],
    "Découpage des moments": lambda v: 350,
    "Source 4K": lambda v: 400 + 120 * 4 * _mpx(v["work"]) + 14 * _mpx(v["source"]),
    "Rendu 4K": lambda v: 1100 + v.get("workers", render_workers(True)) * (
        400 + 1320 * _mpx(v["work"]) + 0.85 * v["montage"]) + 4 * 2.3 * v["montage"],
}
# Opérations de chaque étape des tâches de l'interface locale (noms de local_server.py).
STEPS = {
    "préparation du rush": ["Version de travail (MP4)", "Extraction du son", "Détection de la parole",
                            "Transcription", "Position du visage", "Bruitages et montage"],
    "consignes de montage (Claude)": ["Consignes de montage (Claude)"],
    "montage selon les consignes": ["Montage selon les consignes"],
    "rendu de la version de travail": ["Rendu de la vidéo"], "rendu 1080p": ["Rendu de la vidéo"],
    "montage": ["Montage selon les consignes"],
    "page des moments": ["Découpage des moments"], "mise à jour de la page": ["Découpage des moments"],
    "source 4K": ["Source 4K"], "rendu 4K": ["Rendu 4K"],
}


def video_info(paths, montage=None):
    """Durée totale et tailles d'image d'un rush (une ou plusieurs vidéos, la première donne le format)."""
    import imageio_ffmpeg
    duration = 0.0
    for p in paths:
        out = subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-hide_banner", "-i", p],
                             capture_output=True, text=True).stderr
        m = re.search(r"Duration: (\d+):(\d+):([\d.]+)", out)
        if m:
            duration += int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3])
    source = display_size(paths[0]) if paths else None
    work = work_size(*source) if source else (1080, 1920)
    return {"duration": duration, "source": source or work, "work": work,
            "montage": montage if montage is not None else 0.6 * duration, "parts": len(paths)}


def model(op, info):
    """Besoin d'après la formule seule, en octets."""
    return int(OPERATIONS[op](info) * MO)


def load_measures():
    try:
        data = __import__("json").load(open(MEASURES))
        return data if isinstance(data, dict) else {}
    except (OSError, ValueError):
        return {}


def factor(op, measures=None):
    """(facteur appris, nombre de mesures) : plus grand rapport mesuré / prévu des 5 dernières
    mesures, avec 10 % de marge ; (1, 0) sans mesure."""
    obs = [o for o in (measures if measures is not None else load_measures()).get(op, [])
           if o.get("model") and o.get("peak")][-5:]
    if not obs:
        return 1.0, 0
    return min(3.0, max(0.5, 1.1 * max(o["peak"] / o["model"] for o in obs))), len(obs)


def need(op, info, measures=None):
    """Besoin prévu en octets : formule × facteur appris sur cette machine."""
    return int(model(op, info) * factor(op, measures)[0])


def record(op, peak, info):
    """Garde le pic mesuré d'une opération terminée, avec le besoin prévu par la formule."""
    import datetime
    import json
    data = load_measures()
    obs = data.setdefault(op, [])
    obs.append({"peak": int(peak), "model": model(op, info), "duration": round(info.get("duration", 0)),
                "work": list(info.get("work") or []), "at": datetime.datetime.now().isoformat(timespec="seconds")})
    data[op] = obs[-KEEP_MEASURES:]
    os.makedirs(os.path.dirname(MEASURES), exist_ok=True)
    json.dump(data, open(MEASURES + ".part", "w"), indent=1)
    os.replace(MEASURES + ".part", MEASURES)


def tree_memory(root):
    """Mémoire utilisée par un programme et tout ce qu'il a lancé (somme des PSS, en octets) : une
    étape de l'interface locale. Par filiation, pas par session : les navigateurs du rendu ont la
    leur. 0 si illisible (hors Linux)."""
    children = {}
    try:
        pids = [p for p in os.listdir("/proc") if p.isdigit()]
    except OSError:
        return 0
    for pid in pids:
        try:
            ppid = int(open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()[1])
            children.setdefault(ppid, []).append(int(pid))
        except (OSError, ValueError, IndexError):
            continue
    total, todo = 0, [root]
    while todo:
        pid = todo.pop()
        todo += children.get(pid, [])
        try:
            for line in open(f"/proc/{pid}/smaps_rollup"):
                if line.startswith("Pss:"):
                    total += int(line.split()[1]) * 1024
                    break
        except (OSError, ValueError):
            continue
    return total


RENDERS = {"Rendu de la vidéo": False, "Rendu 4K": True}


def choose_workers(hd, info, avail=None, measures=None):
    """Navigateurs du rendu : le plus possible (4, 2 en 4K, au plus un par cœur) tant que le besoin
    tient dans la mémoire libre ; 1 au moins."""
    top = render_workers(hd)
    if avail is None:
        avail = available()[0]
    if avail is None:
        return top
    op = "Rendu 4K" if hd else "Rendu de la vidéo"
    for w in range(top, 1, -1):
        if need(op, {**info, "workers": w}, measures) <= avail:
            return w
    return 1


def operations(steps, info, avail=None):
    """[(étape, opération, besoin en octets)] des étapes données, dans l'ordre. Rendu : avec le
    nombre de navigateurs qu'il prendra d'après la mémoire libre (choose_workers)."""
    out = []
    measures = load_measures()
    for step in steps:
        ops = list(STEPS.get(step, []))
        if step == "préparation du rush" and info.get("parts", 1) > 1:
            ops.insert(0, "Assemblage des vidéos (réencodage)")
        for op in ops:
            v = info
            if op in RENDERS and "workers" not in info:
                v = {**info, "workers": choose_workers(RENDERS[op], info, avail, measures)}
            out.append((step, op, need(op, v, measures)))
    return out


def available():
    """(mémoire disponible, mémoire totale) en octets : celle de la machine (ou de la machine
    virtuelle de Docker), bornée par la limite du conteneur. (None, None) si illisible."""
    try:
        info = {k: int(v.split()[0]) * 1024 for k, v in (l.split(":", 1) for l in open("/proc/meminfo"))}
        avail, total = info["MemAvailable"], info["MemTotal"]
    except (OSError, KeyError, ValueError):
        return None, None
    # Limite du conteneur : cgroup v2, puis v1 (le cache de fichiers inactif se libère).
    for limit_f, usage_f, stat_f, key in (
            ("/sys/fs/cgroup/memory.max", "/sys/fs/cgroup/memory.current", "/sys/fs/cgroup/memory.stat", "inactive_file"),
            ("/sys/fs/cgroup/memory/memory.limit_in_bytes", "/sys/fs/cgroup/memory/memory.usage_in_bytes",
             "/sys/fs/cgroup/memory/memory.stat", "total_inactive_file")):
        try:
            limit = open(limit_f).read().strip()
            if not limit.isdigit() or int(limit) >= total:
                continue
            usage = int(open(usage_f).read())
            stat = dict(l.split() for l in open(stat_f) if len(l.split()) == 2)
            free = int(limit) - usage + int(stat.get(key, 0))
            return max(0, min(avail, free)), int(limit)
        except (OSError, ValueError):
            continue
    return avail, total


def go(n):
    return f"{n / 2**30:.1f} Go".replace(".", ",")


def verdict(need, avail):
    """« ok », « juste » (moins de 25 % de marge) ou « insuffisant »."""
    if avail is None:
        return "ok"
    return "insuffisant" if avail < need else "juste" if avail < need * MARGIN else "ok"


def advice(level, need, avail, what):
    """Phrase d'avertissement pour l'utilisateur (vide si la mémoire suffit)."""
    if level == "ok":
        return ""
    head = (f"Mémoire insuffisante pour {what} : il faut environ {go(need)}, il n'y a que {go(avail)} de libres."
            if level == "insuffisant" else
            f"Mémoire juste pour {what} : il faut environ {go(need)}, il y a {go(avail)} de libres.")
    return (head + " L'étape risque d'être arrêtée en route. Fermez d'autres programmes, ou donnez plus de "
            "mémoire à Docker (Réglages > Resources > Memory), puis relancez ou reprenez la tâche.")


def summary(steps, info):
    """Bilan pour une tâche : besoin de chaque opération, pic, mémoire libre et avertissement."""
    avail, total = available()
    ops = operations(steps, info, avail)
    peak = max(ops, key=lambda o: o[2]) if ops else (None, None, 0)
    level = verdict(peak[2], avail)
    measures = load_measures()
    return {"operations": [{"step": s, "op": o, "need": n, "level": verdict(n, avail),
                            "measured": factor(o, measures)[1]} for s, o, n in ops],
            "peak": peak[2], "peakOp": peak[1], "available": avail, "total": total, "level": level,
            "message": advice(level, peak[2], avail, f"l'étape « {peak[1]} »") if peak[1] else ""}


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    s = summary(["préparation du rush"], video_info(sys.argv[1:]))
    print(f"Mémoire : environ {go(s['peak'])} au plus fort de la préparation ({s['peakOp']}), "
          + (f"{go(s['available'])} libres sur {go(s['total'])}." if s["available"] is not None else "mémoire libre inconnue."))
    for o in s["operations"]:
        print(f"   {o['op']} : {go(o['need'])}" + ("" if o["level"] == "ok" else f" ({o['level']})"))
    if s["message"]:
        print(f"ATTENTION : {s['message']}")
