"""Applique une variante choisie sur la page des moments.

Usage : python3 scripts/use_selection.py <dossier d'export> <variante>
<dossier d'export> = ce que Claude a exporté de la page (ArtifactData avec out_dir) :
  variantes/<variante>.json  {"name", "rush", "keep": [ids de segments], "updatedAt"}
  moments/<id>.json          {"rush", "text"?: texte corrigé, "nosub"?: true (sans sous-titres),
                              "sfx"?: ["cash", …] ou [{"kind", "at", "repeat", "gap", "dur"}, …],
                              "vfx"?: [{"kind": "typed" | "chip", "text", "at", "type", "dur", "sound"}, …],
                              "trim"?: {"in", "out"} pour un plan sans parole (id >= 1000),
                              "cuts"?: [instants de coupe en s], "fxv"?: 2}
  Dans « keep », « id/k » désigne la partie k d'un moment découpé (« Couper ici »).
Écrit work/selection.json, que build_edit.py utilise à la place de KEEP, avec les
corrections de sous-titres et les effets sonores choisis par moment.
"""
import glob
import json
import os
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
export_dir, variant = sys.argv[1], sys.argv[2]
rush = os.path.basename(open(os.path.join(ROOT, "work", "rush.txt")).read().strip())


def load(path):
    doc = json.load(open(path))
    return doc.get("data", doc)  # export brut ({id, data, version}) ou données seules


doc = load(os.path.join(export_dir, "variantes", f"{variant}.json"))
if doc.get("rush") != rush:
    sys.exit(f"Cette sélection vise {doc.get('rush')!r}, le rush en cours est {rush!r}")

corrections, sfx, vfx, trims, cuts, sfx_legacy, gains = {}, {}, {}, {}, {}, [], {}
for path in glob.glob(os.path.join(export_dir, "moments", "*.json")):
    m = load(path)
    if m.get("rush") != rush:
        continue
    seg = os.path.splitext(os.path.basename(path))[0]
    if m.get("nosub") is True:  # sous-titres supprimés sur la page : texte vide
        corrections[seg] = ""
    elif isinstance(m.get("text"), str) and m["text"].strip():
        corrections[seg] = m["text"].strip()
    if isinstance(m.get("sfx"), list):
        sfx[seg] = [s for s in m["sfx"] if isinstance(s, (str, dict))]
        if m.get("fxv") != 2:  # enregistré avant l'effet « voix » : voix par défaut gardée
            sfx_legacy.append(seg)
    if isinstance(m.get("cuts"), list) and m["cuts"]:
        cuts[seg] = sorted(float(c) for c in m["cuts"] if isinstance(c, (int, float)))
    if isinstance(m.get("trim"), dict):  # plan sans parole : {"in", "out"} en s
        trims[seg] = {k: float(m["trim"][k]) for k in ("in", "out") if isinstance(m["trim"].get(k), (int, float))}
    if isinstance(m.get("gain"), (int, float)) and m["gain"] != 1:  # volume de la voix du moment
        gains[seg] = max(0.0, min(2.0, float(m["gain"])))
    if isinstance(m.get("vfx"), list):
        vfx[seg] = [v for v in m["vfx"] if isinstance(v, dict)]

json.dump(
    {"name": doc.get("name"), "keep": doc["keep"], "corrections": corrections, "sfx": sfx, "vfx": vfx,
     "trims": trims, "cuts": cuts, "sfx_legacy": sfx_legacy, "gains": gains},
    open(os.path.join(ROOT, "work", "selection.json"), "w"),
    ensure_ascii=False,
    indent=1,
)
print(f"Variante « {doc.get('name')} » : {len(doc['keep'])} moments, "
      f"{len(corrections)} textes corrigés, {len(sfx)} moments avec effets sonores et "
      f"{len(vfx)} avec effets visuels choisis, {len(cuts)} découpés -> work/selection.json")
