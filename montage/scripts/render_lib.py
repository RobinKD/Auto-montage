"""Ce que render_hyperframes.py prépare en Python : montage (src/data/edit.json, face.json),
navigateur de rendu, bruitages, police des sous-titres, mots mis en valeur et coupure des
sous-titres en 2 lignes (mesurée dans la police). Les effets image par image (zooms centrés sur
le visage, secousses, flashs, ressort des apparitions) sont dans la page HyperFrames, en
JavaScript ; la page des moments les imite en direct.

Unités : pixels CSS de l'image de la composition, 1080 x 1920 (portrait) ou 1920 x 1080 (rush en
paysage) ; le rendu 4K multiplie par 2.
"""
import itertools
import json
import os
import re
import subprocess
import unicodedata

import fonts_lib

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
PUBLIC = os.path.join(ROOT, "public")
CAPTION_MARGIN = 140  # sous-titres : largeur de l'image moins cette marge (940 px sur 1080)
HIGHLIGHT_SCALE = 1.2
POPPINS = os.path.join(PUBLIC, "fonts", "Poppins-Black-latin.ttf")


def headless_chrome():
    """Chrome sans écran : celui de l'environnement cloud, sinon celui que trouve ou télécharge
    scripts/ensure_browser.sh (Playwright sur Linux ARM64, « hyperframes browser ensure » ailleurs)."""
    out = subprocess.run(["bash", os.path.join(ROOT, "scripts", "ensure_browser.sh")], cwd=ROOT,
                         capture_output=True, text=True).stdout
    # Le chemin du navigateur : dernier chemin affiché qui est un fichier exécutable.
    paths = [w for line in out.splitlines() for w in line.split() if os.path.isabs(w)]
    return next((p for p in reversed(paths) if os.path.isfile(p) and os.access(p, os.X_OK)), None)


def norm(w):
    """Mot comparé sans accents ni ponctuation (comme norm() de la page des moments)."""
    w = unicodedata.normalize("NFD", w.lower())
    return re.sub(r"[^a-z0-9]+", "", "".join(c for c in w if not unicodedata.combining(c)))


# --- Polices ------------------------------------------------------------------------------

def caption_font(cs):
    """Police des sous-titres : (fichier, graisse du fichier) pour la graisse demandée, comme le
    navigateur (fichier le plus proche ; un gras absent est simulé). Sans style choisi : Oliver
    (déclarée en 400, gras simulé), sinon Patrick Hand (déclarée en 700, sans gras simulé)."""
    files = [(os.path.join(PUBLIC, f["url"]), int(f["weight"])) for f in (cs or {}).get("files") or []]
    files = [f for f in files if os.path.exists(f[0])]
    if files:
        return min(files, key=lambda f: (abs(f[1] - cs["weight"]), f[1]))
    fonts = os.path.join(PUBLIC, "fonts")
    oliver = next((os.path.join(fonts, n) for n in sorted(os.listdir(fonts))
                   if re.fullmatch(r"oliver[^/]*\.(ttf|otf|woff2?)", n, re.I)), None)
    return (oliver, 400) if oliver else (os.path.join(fonts, "PatrickHand-latin.woff2"), 700)


# --- Montage --------------------------------------------------------------------------------
class Montage:
    def __init__(self, hd=False):
        data = os.path.join(ROOT, "src", "data")
        self.edit = json.load(open(os.path.join(data, "edit.json")))
        self.face = json.load(open(os.path.join(data, "face.json")))
        self.hd = hd
        self.fps = self.edit.get("fps", 30)
        self.frames = self.edit["durationInFrames"]
        self.width, self.height = int(self.edit.get("width") or 1080), int(self.edit.get("height") or 1920)
        self.landscape = self.width > self.height
        e = self.edit
        self.hab = {"autoZoom": 1, "pop": False, "karaoke": False, "karaokeColor": "#ffd84d", **(e.get("habillage") or {})}
        self.typed = e.get("typed") or []
        cs = e.get("captionStyle") or {}
        self.font_file, self.font_weight = caption_font(cs)
        # Sans style choisi : Oliver en gras (ou Patrick Hand si Oliver manque).
        self.cap = {"weight": 700, "size": 64, "uppercase": True, "color": "#f4e2c2", "shadow": 50, "blur": 14,
                    **{k: v for k, v in cs.items() if v is not None}}
        self.cap["ox"], self.cap["oy"] = fonts_lib.shadow_offset(self.cap)
        self._font = None

    @property
    def rush(self):
        """Source vidéo et position de chaque plan (rush 1080 entier, ou source 4K des images gardées)."""
        if self.hd:
            return os.path.join(PUBLIC, "rushes", "rush_2160.webm"), "trimBeforeHd"
        return os.path.join(PUBLIC, "rushes", "rush_1080.mp4"), "trimBefore"


    def sfx(self):
        """Bruitages : (fichier, début en s calé sur l'image, durée jouée, volume)."""
        out = []
        for s in self.edit.get("sfx") or []:
            start = round(s["start"] * self.fps) / self.fps
            dur = max(1, round(self.fps * s.get("dur", 1.5))) / self.fps
            out.append((os.path.join(PUBLIC, s["src"]), start, dur, s.get("volume", 1)))
        return out

    # --- Sous-titres ---
    def measure(self):
        """Largeur d'un mot (px CSS, taille de la police des sous-titres), majuscules appliquées."""
        if self._font is None:
            from PIL import ImageFont
            self._font = ImageFont.truetype(self.font_file, self.cap["size"])
        up = self.cap["uppercase"]
        return lambda w: self._font.getlength(w.upper() if up else w)

    def caption_words(self, c):
        return c.get("words") or " ".join(x for x in (c.get("top"), c.get("bottom")) if x).split(" ")

    def caption_colors(self, c, words):
        """Couleur de chaque mot mis en valeur sur ce moment (None sinon)."""
        hl = [h for h in self.edit.get("highlights") or [] if h["start"] - 0.01 <= c["start"] < h["end"]]
        out = []
        for w in words:
            mine = {norm(x) for x in w.split()}
            h = next((h for h in hl if any(n and n in mine for n in map(norm, h["words"].split()))), None)
            out.append(h["color"] if h else None)
        return out

    def caption_layout(self, words, big):
        """(lignes [(début, fin, taille relative)], réduction) : au plus « lines » lignes, coupées là où
        leurs largeurs (multipliées par la taille de chaque ligne) sont les plus proches ; réduites
        ensemble si la plus large dépasse. Même règle dans la page des moments (captionLayout)."""
        one = self.measure()
        space = one(" ")
        widths = [one(w) * (HIGHLIGHT_SCALE if big[i] else 1) for i, w in enumerate(words)]

        def width(a, b):
            return sum(widths[a:b]) + space * max(0, b - a - 1)
        n = max(1, min(int(self.cap.get("lines") or 2), len(words)))
        sizes = [s / 100 for s in (self.cap.get("lineSizes") or [100, 100, 100])][:n]
        sizes += [1.0] * (n - len(sizes))
        best, lines = None, [(0, len(words), sizes[0])]
        for cuts in itertools.combinations(range(1, len(words)), n - 1):
            bounds = [0, *cuts, len(words)]
            w = [width(a, b) * sizes[i] for i, (a, b) in enumerate(zip(bounds, bounds[1:]))]
            key = (round(max(w) - min(w), 3), max(w))
            if best is None or key < best:
                best, lines = key, [(a, b, sizes[i]) for i, (a, b) in enumerate(zip(bounds, bounds[1:]))]
        widest = max(width(a, b) * s for a, b, s in lines)
        return lines, min(1.0, (self.width - CAPTION_MARGIN) / max(1.0, widest))

