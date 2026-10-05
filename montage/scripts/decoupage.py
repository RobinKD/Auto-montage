"""Scinder ou fusionner des moments de parole (page des moments, interface locale).

Usage : python3 scripts/decoupage.py merge <id>        fusionne le moment avec le suivant
        python3 scripts/decoupage.py split <id> <t>    scinde à l'instant t (s depuis le début
                                                       de l'extrait du moment ; coupe entre deux mots)
        python3 scripts/decoupage.py unmerge <id>      sépare un moment fusionné
        python3 scripts/decoupage.py unsplit <id>      recolle une partie au moment précédent
Écrit work/decoupage.json (moments_lib : la transcription ne change pas, les ids restent
stables) et reporte les réglages de la page (work/local_db : moments gardés de chaque variante,
effets, corrections) sur les nouveaux moments. Ensuite : make_moments.py (extraits et page).
"""
import datetime
import json
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from moments_lib import effective_segments, load_segments, save_decoupage  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
WORK = os.path.join(ROOT, "work")
DB = os.path.join(WORK, "local_db")


class Refus(Exception):
    """Action impossible, avec un message pour l'utilisateur."""


def base(m):
    """Début de l'extrait d'un moment sur la page (les temps des effets en partent)."""
    return max(0.0, m["start"] - 0.05)


def doc_path(mid):
    return os.path.join(DB, "moments", f"{mid}.json")


def read_doc(mid):
    try:
        d = json.load(open(doc_path(mid)))
        return d.get("data", d) if isinstance(d, dict) else {}
    except (OSError, ValueError):
        return {}


def write_doc(mid, d):
    os.makedirs(os.path.dirname(doc_path(mid)), exist_ok=True)
    d = {**d, "fxv": 2, "updatedAt": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    json.dump(d, open(doc_path(mid), "w"), ensure_ascii=False, indent=1)


def shifted(items, dt, keep=lambda at: True):
    """Effets (sons ou visuels) décalés de dt secondes ; seulement ceux dont le départ convient."""
    out = []
    for f in items or []:
        if isinstance(f, dict) and "at" in f:
            at = float(f.get("at") or 0)
            if keep(at):
                out.append({**f, "at": round(max(0.0, at + dt), 3)})
        elif keep(0.0):
            out.append(f)  # effet sans réglage (nom seul) : au début, comme avant
    return out


def variants():
    folder = os.path.join(DB, "variantes")
    for n in sorted(os.listdir(folder)) if os.path.isdir(folder) else []:
        if n.endswith(".json"):
            path = os.path.join(folder, n)
            try:
                d = json.load(open(path))
            except (OSError, ValueError):
                continue
            yield path, d


def kept_in(keep, mid):
    return any(x == mid or (isinstance(x, str) and x.split("/")[0] == str(mid)) for x in keep)


def update_keep(fn):
    """fn(liste keep) -> nouvelle liste, pour chaque variante."""
    for path, d in variants():
        data = d.get("data", d)
        if isinstance(data.get("keep"), list):
            data["keep"] = fn(list(data["keep"]))
            json.dump(d, open(path, "w"), ensure_ascii=False, indent=1)


def state():
    raw, eff, deco = load_segments(WORK)
    return raw, eff, deco, {m["id"]: i for i, m in enumerate(eff)}


def merge(mid):
    raw, eff, deco, pos = state()
    if mid not in pos:
        raise Refus("Moment introuvable.")
    i = pos[mid]
    if i + 1 >= len(eff):
        raise Refus("C'est le dernier moment : rien à fusionner après lui.")
    a, b = eff[i], eff[i + 1]
    members = (a.get("merged") or [a["id"]]) + (b.get("merged") or [b["id"]])
    deco["merges"] = [g for g in deco["merges"] if g[0] not in (a["id"], b["id"])] + [members]
    save_decoupage(WORK, deco)
    # Réglages : ceux du 1er moment, plus ceux du suivant décalés ; textes corrigés mis bout à bout.
    da, db = read_doc(a["id"]), read_doc(b["id"])
    dt = base(b) - base(a)
    out = {**da, "rush": da.get("rush") or db.get("rush")}
    for key in ("sfx", "vfx"):
        if isinstance(db.get(key), list):
            out[key] = (da.get(key) if isinstance(da.get(key), list) else []) + shifted(db[key], dt)
    if da.get("text") or db.get("text"):
        out["text"] = f"{da.get('text') or a['text']} {db.get('text') or b['text']}".strip()
    out["nosub"] = True if da.get("nosub") and db.get("nosub") else None
    out["cuts"] = None  # coupes « Couper ici » : à refaire sur le moment entier
    if out.get("rush"):
        write_doc(a["id"], out)
    update_keep(lambda keep: [x for x in keep if not (kept_in([x], a["id"]) or kept_in([x], b["id"]))]
                + ([a["id"]] if kept_in(keep, a["id"]) or kept_in(keep, b["id"]) else []))
    return f"Moments fusionnés : « {a['text'][:40]} » + « {b['text'][:40]} »"


def unmerge(mid):
    raw, eff, deco, pos = state()
    m = eff[pos[mid]] if mid in pos else None
    if not m or not m.get("merged"):
        raise Refus("Ce moment n'est pas une fusion.")
    deco["merges"] = [g for g in deco["merges"] if g[0] != mid]
    save_decoupage(WORK, deco)
    # Le 1er moment retrouve sa durée : ses effets au-delà (venus des autres) sont retirés, les
    # autres moments retrouvent leurs propres réglages.
    first = next(x for x in effective_segments(raw, deco) if x["id"] == mid)
    length = first["end"] + 0.05 - base(first)
    d = read_doc(mid)
    if d:
        for key in ("sfx", "vfx"):
            if isinstance(d.get(key), list):
                d[key] = shifted(d[key], 0, keep=lambda at: at < length)
        d["text"] = None  # texte corrigé de l'ensemble : ne correspond plus
        write_doc(mid, d)
    members = m["merged"]
    update_keep(lambda keep: keep + [x for x in members[1:] if x not in keep] if kept_in(keep, mid) else keep)
    return "Moment séparé en ses moments d'origine."


def split(mid, t):
    raw, eff, deco, pos = state()
    if mid not in pos:
        raise Refus("Moment introuvable.")
    m = eff[pos[mid]]
    words = m["words"]
    if len(words) < 2:
        raise Refus("Un seul mot dans ce moment : rien à scinder.")
    at = base(m) + float(t)
    w = min(range(1, len(words)), key=lambda k: abs(words[k]["a"] - at))
    o, oi = words[w]["o"], words[w]["oi"]
    # Parties sans les fusions : la partie qui contient le mot de coupe.
    parts = effective_segments(raw, {"splits": deco["splits"], "merges": []})
    members = m.get("merged") or [m["id"]]
    holder = next(p for p in parts if p["id"] in members and any(x["o"] == o and x["oi"] == oi for x in p["words"]))
    at_boundary = holder["words"][0]["o"] == o and holder["words"][0]["oi"] == oi
    if at_boundary:
        k = members.index(holder["id"])
        groups = [members[:k], members[k:]]
        second = holder["id"]
    else:
        deco["splits"][str(o)] = sorted(set(deco["splits"].get(str(o), [])) | {oi})
        from moments_lib import part_id
        second = part_id(o, oi)
        k = members.index(holder["id"]) + 1
        groups = [members[:k], [second] + members[k:]]
    deco["merges"] = [g for g in deco["merges"] if g[0] != m["id"]] + [g for g in groups if len(g) > 1]
    save_decoupage(WORK, deco)
    new = {x["id"]: x for x in effective_segments(raw, deco)}
    t_cut = words[w]["a"] - base(m)
    dt = base(new[second]) - base(m)
    d = read_doc(m["id"])
    if d:
        moved = {key: shifted(d.get(key), -dt, keep=lambda x: x >= t_cut) for key in ("sfx", "vfx") if isinstance(d.get(key), list)}
        for key in moved:
            d[key] = shifted(d[key], 0, keep=lambda x: x < t_cut)
        d["text"], d["cuts"] = None, None
        write_doc(m["id"], d)
        if not at_boundary and moved:
            write_doc(second, {"rush": d.get("rush"), **moved})
    update_keep(lambda keep: keep + [second] if kept_in(keep, m["id"]) and second not in keep else keep)
    return f"Moment scindé avant « {words[w]['w']} »."


def unsplit(mid):
    raw, eff, deco, pos = state()
    m = eff[pos[mid]] if mid in pos else None
    if not m or m.get("splitFrom") is None or m.get("merged"):
        raise Refus("Cette partie ne peut pas être recollée (pas une partie scindée, ou fusionnée).")
    o, k = m["splitFrom"], m["words"][0]["oi"]
    prev = eff[pos[mid] - 1] if pos[mid] > 0 else None
    deco["splits"][str(o)] = [x for x in deco["splits"].get(str(o), []) if x != k]
    save_decoupage(WORK, deco)
    if prev and any(w["o"] == o for w in prev["words"]):
        dp, dm = read_doc(prev["id"]), read_doc(mid)
        dt = base(m) - base(prev)
        for key in ("sfx", "vfx"):
            if isinstance(dm.get(key), list):
                dp[key] = (dp.get(key) if isinstance(dp.get(key), list) else []) + shifted(dm[key], dt)
        if dp.get("rush") or dm.get("rush"):
            dp["rush"] = dp.get("rush") or dm.get("rush")
            dp["text"], dp["cuts"] = None, None
            write_doc(prev["id"], dp)
        update_keep(lambda keep: [x for x in keep if not kept_in([x], mid)]
                    + ([prev["id"]] if kept_in(keep, mid) and not kept_in(keep, prev["id"]) else []))
    return "Partie recollée au moment précédent."


def main():
    action, args = sys.argv[1], sys.argv[2:]
    fn = {"merge": merge, "unmerge": unmerge, "split": split, "unsplit": unsplit}[action]
    try:
        print(fn(int(args[0]), *[float(x) for x in args[1:]]))
    except Refus as e:
        sys.exit(str(e))


if __name__ == "__main__":
    main()
