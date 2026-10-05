"""Polices utilisables pour les sous-titres, et style choisi.

Polices proposées (celles que le rendu peut vraiment utiliser) :
  - « Auto-montage » : public/fonts/ (Oliver, Patrick Hand, Poppins…) ;
  - « Vos polices »  : public/fonts/perso/, ajoutées depuis la page des moments ;
  - « Système »      : polices installées là où tourne le rendu (fc-list : celles du
                       conteneur en local, pas celles du Mac ou du PC).
Formats lisibles par le navigateur de rendu seulement (ttf, otf, woff, woff2), sans italique.

Style des sous-titres : work/caption_style.json (montage en cours ; mis de côté et rouvert
avec lui), et work/caption_style_default.json (dernier style choisi, repris par les montages
suivants). Forme : {"font": "<id de famille>", "weight": 700, "size": 64,
"uppercase": true, "color": "#f4e2c2", "shadow": 50, "blur": 14, "ox": 0, "oy": 0} (ombre :
intensité de 0 à 100 (plus ou moins sombre), diffusion du flou en pixels de 0 à 60, et décalage
ox (vers la droite) et oy (vers le bas) en pixels, à 40 px au plus du texte, choisi dans le cercle
de la page des moments ; tout en pixels sur une image de 1080 px de large ; l'ancienne direction
dx/dy de -1 à 1, d'avant la 0.42, est convertie en décalage). build_edit.py le copie dans edit.json
(« captionStyle ») avec les fichiers de la police, copiés dans public/fonts/choisie/.
"""
import hashlib
import json
import os
import re
import shutil
import subprocess

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
WORK = os.path.join(ROOT, "work")
FONTS = os.path.join(ROOT, "public", "fonts")
PERSO = os.path.join(FONTS, "perso")
CHOSEN = os.path.join(FONTS, "choisie")
STYLE = os.path.join(WORK, "caption_style.json")
STYLE_DEFAULT = os.path.join(WORK, "caption_style_default.json")
FONT_EXT = (".ttf", ".otf", ".woff", ".woff2")
# Style d'origine : Oliver (ou Patrick Hand) en gras, crème, en majuscules.
DEFAULT = {"font": None, "weight": 700, "size": 64, "uppercase": True, "color": "#f4e2c2", "shadow": 50, "blur": 14,
           "ox": 0, "oy": 0}
SHADOW_RANGE, BLUR_RANGE, OFFSET_MAX = (0, 100), (0, 60), 40
SIZE_RANGE = (36, 120)
# Graisses de fontconfig -> CSS.
FC_WEIGHTS = [(0, 100), (40, 200), (50, 300), (80, 400), (100, 500), (180, 600), (200, 700), (205, 800), (210, 900)]


def css_weight(fc):
    return min(FC_WEIGHTS, key=lambda p: abs(p[0] - fc))[1]


def guess_from_name(path):
    """Famille et graisse d'après le nom du fichier (sans fontconfig)."""
    stem = os.path.splitext(os.path.basename(path))[0]
    stem = re.sub(r"[-_](latin|subset)$", "", stem, flags=re.I)
    weights = [("thin", 100), ("extralight", 200), ("light", 300), ("regular", 400), ("medium", 500),
               ("semibold", 600), ("extrabold", 800), ("bold", 700), ("black", 900), ("heavy", 900)]
    weight = 400
    family = stem
    m = re.match(r"^(.*?)[-_ ]?(thin|extralight|light|regular|medium|semibold|extrabold|bold|black|heavy)$", stem, re.I)
    if m:
        family = m.group(1)
        weight = dict(weights)[m.group(2).lower()]
    family = re.sub(r"(?<=[a-z])(?=[A-Z])", " ", family).replace("_", " ").replace("-", " ").strip()
    return family or stem, weight, "italic" in stem.lower() or "oblique" in stem.lower()


def query(path):
    """(famille, graisse CSS, italique) d'un fichier, par fc-query si possible."""
    try:
        out = subprocess.run(["fc-query", "--format", "%{family[0]}|%{weight}|%{slant}\n", path],
                             capture_output=True, text=True, timeout=10).stdout.splitlines()
        fam, w, slant = out[0].split("|")
        return fam, css_weight(float(w)), float(slant or 0) > 0
    except (OSError, IndexError, ValueError, subprocess.SubprocessError):
        return guess_from_name(path)


def family_id(source, family):
    return re.sub(r"[^a-z0-9]+", "-", f"{source} {family}".lower()).strip("-")[:60]


def file_id(path):
    return hashlib.sha1(os.path.realpath(path).encode()).hexdigest()[:16]


def _add(families, source, label, path, family, weight):
    fid = family_id(source, family)
    fam = families.setdefault(fid, {"id": fid, "family": family, "source": label, "faces": {}})
    fam["faces"].setdefault(weight, path)  # une seule face par graisse (la première trouvée)


def list_fonts():
    """Familles utilisables : [{id, family, source, faces: {graisse: chemin}}], triées."""
    families = {}
    for folder, source, label in ((FONTS, "am", "Auto-montage"), (PERSO, "perso", "Vos polices")):
        for name in sorted(os.listdir(folder)) if os.path.isdir(folder) else []:
            path = os.path.join(folder, name)
            if os.path.isfile(path) and name.lower().endswith(FONT_EXT) and not name.startswith("."):
                family, weight, italic = query(path)
                if not italic:
                    _add(families, source, label, path, family, weight)
    try:
        out = subprocess.run(["fc-list", "--format", "%{family[0]}|%{weight}|%{slant}|%{file}\n"],
                             capture_output=True, text=True, timeout=20).stdout
    except (OSError, subprocess.SubprocessError):
        out = ""
    for line in sorted(out.splitlines()):
        parts = line.split("|")
        if len(parts) != 4 or not parts[3].lower().endswith(FONT_EXT):
            continue
        family, w, slant, path = parts
        if path.startswith(FONTS + os.sep) or float(slant or 0) > 0:
            continue
        try:
            _add(families, "sys", "Système", path, family, css_weight(float(w)))
        except ValueError:
            continue
    order = {"Auto-montage": 0, "Vos polices": 1, "Système": 2}
    return sorted(families.values(), key=lambda f: (order[f["source"]], f["family"].lower()))


def default_font_id(fonts):
    """Police d'origine des sous-titres : Oliver si présente, sinon Patrick Hand."""
    for wanted in ("oliver", "patrick hand"):
        for f in fonts:
            if f["source"] == "Auto-montage" and f["family"].lower().startswith(wanted):
                return f["id"]
    return fonts[0]["id"] if fonts else None


def read_style():
    for path in (STYLE, STYLE_DEFAULT):
        try:
            data = json.load(open(path))
            if isinstance(data, dict):
                return clean(data)
        except (OSError, ValueError):
            pass
    return dict(DEFAULT)


def clean(data):
    style = dict(DEFAULT)
    if isinstance(data.get("font"), str) and re.fullmatch(r"[a-z0-9-]{1,60}", data["font"]):
        style["font"] = data["font"]
    try:
        style["weight"] = max(100, min(900, int(round(float(data.get("weight", 700)) / 100) * 100)))
        style["size"] = max(SIZE_RANGE[0], min(SIZE_RANGE[1], int(round(float(data.get("size", 64))))))
    except (TypeError, ValueError):
        pass
    style["uppercase"] = bool(data.get("uppercase", True))
    if isinstance(data.get("color"), str) and re.fullmatch(r"#[0-9a-fA-F]{6}", data["color"]):
        style["color"] = data["color"].lower()
    for key, (lo, hi) in (("shadow", SHADOW_RANGE), ("blur", BLUR_RANGE)):
        try:
            style[key] = max(lo, min(hi, int(round(float(data.get(key, DEFAULT[key]))))))
        except (TypeError, ValueError):
            pass
    style["ox"], style["oy"] = shadow_offset(data | {"shadow": style["shadow"], "blur": style["blur"]})
    return style


def shadow_offset(style):
    """Décalage de l'ombre (ox, oy) en pixels, dans le cercle de rayon OFFSET_MAX. Un style d'avant la 0.42
    n'a qu'une direction dx/dy (-1 à 1) : décalage de 3 px à 50 % d'intensité, plus la moitié du flou."""
    try:
        if "ox" in style or "oy" in style:
            ox, oy = float(style.get("ox") or 0), float(style.get("oy") or 0)
        else:
            dx, dy = float(style.get("dx") or 0), float(style.get("dy") or 0)
            n = (dx * dx + dy * dy) ** 0.5
            d = 3 * float(style.get("shadow", 50)) / 50 + float(style.get("blur", 14)) / 2
            ox, oy = (dx / n * d, dy / n * d) if n else (0, 0)
    except (TypeError, ValueError):
        return 0, 0
    r = (ox * ox + oy * oy) ** 0.5
    if r > OFFSET_MAX:
        ox, oy = ox * OFFSET_MAX / r, oy * OFFSET_MAX / r
    return int(round(ox)), int(round(oy))


def save_style(data):
    style = clean(data)
    os.makedirs(WORK, exist_ok=True)
    for path in (STYLE, STYLE_DEFAULT):
        json.dump(style, open(path, "w"), indent=1)
    return style


def resolve(style, fonts=None):
    """Famille du style (ou celle d'origine si elle n'existe plus sur cette machine)."""
    fonts = list_fonts() if fonts is None else fonts
    by_id = {f["id"]: f for f in fonts}
    return by_id.get(style.get("font")) or by_id.get(default_font_id(fonts))


def faces_for(family, weight):
    """Face à charger : la plus proche de la graisse voulue (un gras absent de la police est
    simulé par le navigateur)."""
    faces = family["faces"]
    near = min(faces, key=lambda w: (abs(w - weight), w))
    return sorted({near: faces[near]}.items())


def copy_for_render(style):
    """Copie la police choisie dans public/fonts/choisie/ : {"family", "files", …} pour edit.json."""
    family = resolve(style)
    if os.path.isdir(CHOSEN):
        shutil.rmtree(CHOSEN)
    if not family:
        return None
    os.makedirs(CHOSEN)
    files = []
    for weight, path in faces_for(family, style["weight"]):
        name = f"{weight}{os.path.splitext(path)[1].lower()}"
        shutil.copy(path, os.path.join(CHOSEN, name))
        files.append({"url": f"fonts/choisie/{name}", "weight": str(weight)})
    # Nom de famille propre au rendu : pas de confusion avec une police du système du même nom.
    return {"family": "AM Sous-titres", "label": family["family"], "files": files, "weight": style["weight"], "size": style["size"],
            "uppercase": style["uppercase"], "color": style["color"], "shadow": style["shadow"], "blur": style["blur"],
            "ox": style["ox"], "oy": style["oy"]}


if __name__ == "__main__":  # polices utilisables et style actuel (pour Claude et le dépannage)
    for f in list_fonts():
        print(f"{f['id']:32} {f['family']} ({f['source']}) graisses {', '.join(map(str, sorted(f['faces'])))}")
    print("Style actuel :", json.dumps(read_style(), ensure_ascii=False))
