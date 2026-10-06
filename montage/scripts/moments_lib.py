"""Plans sans parole : les intervalles entre les segments de parole du rush.

Partagé par make_moments.py (page) et build_edit.py (montage) pour qu'ils numérotent
les plans de la même façon : id = GAP_BASE + rang, temps en secondes dans le rush.
"""
import difflib
import json
import os
import re
import wave

GAP_BASE = 1000
MIN_GAP = 0.5  # plus court, ce n'est qu'une respiration entre deux phrases


def rush_duration(work_dir):
    with wave.open(os.path.join(work_dir, "rush_16k.wav")) as w:
        return w.getnframes() / w.getframerate()


# Formats courants : des proportions à moins de 1 % de l'un d'eux (9:16 recadré puis agrandi…)
# prennent exactement ce format.
COMMON_RATIOS = [(9, 16), (16, 9), (4, 3), (3, 4), (1, 1), (4, 5), (5, 4), (2, 3), (3, 2)]


def work_size(w, h, short=1080):
    """Taille de la version de travail pour une image de w x h affichée : mêmes proportions, petit
    côté à `short`, dimensions paires (1080x1920 en 9:16, 1920x1080 en 16:9, 1440x1080 en 4:3…)."""
    near = next(((a, b) for a, b in COMMON_RATIOS if abs((w / h) / (a / b) - 1) < 0.01), None)
    if near:
        w, h = near
    even = lambda x: max(2, int(round(x / 2)) * 2)
    return (short, even(h * short / w)) if w <= h else (even(w * short / h), short)


def res_label(width, height, short):
    """Dimensions d'une version du montage dont le petit côté vaut `short` (« 720x1280 », « 2880x2160 »…)."""
    return "x".join(map(str, work_size(width, height, short)))


def frame_size(root):
    """Format de la composition, celui du rush : (largeur, hauteur) de sa version de travail
    (prepare.sh : 1080x1920 en 9:16, 1920x1080 en 16:9, 1440x1080 en 4:3…). build_edit.py l'écrit dans
    edit.json (« width », « height »), que lisent le rendu, la page des moments et les téléchargements."""
    try:
        import imageio_ffmpeg
        meta = next(imageio_ffmpeg.read_frames(os.path.join(root, "public", "rushes", "rush_1080.mp4")))
        return tuple(meta["size"])
    except (OSError, StopIteration, KeyError, RuntimeError, ValueError, TypeError):
        return 1080, 1920


def frame_layout(width, height):
    """Positions de l'habillage en px de la composition (tailles en px, petit côté de 1080, les mêmes
    dans tous les formats). Image haute (9:16) : sous-titres à 1080 px du haut, texte tapé centré
    sous 520 px ; image basse (16:9, 4:3) : sous-titres à 310 px du bas, texte tapé sous 120 px."""
    return {"capTop": min(1080, height - 310), "typedTop": max(120, height - 1400)}


def display_size(path):
    """Taille de l'image d'une vidéo telle qu'elle s'affiche (rotation du téléphone et pixels non
    carrés compris), lue dans la sortie de « ffmpeg -i »."""
    import re
    import subprocess
    import imageio_ffmpeg
    out = subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-hide_banner", "-i", path], capture_output=True, text=True).stderr
    video = re.search(r"Stream #\d+:\d+.*?: Video: (.*)", out)
    size = video and re.search(r"[ ,](\d{2,5})x(\d{2,5})[ ,\[]", video[1] + " ")
    if not size:
        return None
    w, h = int(size[1]), int(size[2])
    sar = re.search(r"SAR (\d+):(\d+)", video[1])
    if sar and int(sar[1]) and int(sar[2]) and sar[1] != sar[2]:
        w = round(w * int(sar[1]) / int(sar[2]))
    rot = re.search(r"rotation of (-?[\d.]+) degrees", out) or re.search(r"rotate\s*:\s*(-?\d+)", out)
    if rot and round(float(rot[1])) % 180 == 90:
        w, h = h, w
    return w, h


# Langue parlée dans le rush (transcription Whisper) : français sauf choix contraire sur la page
# « Rushes et montages » ou « prepare.sh --langue en ». Fixée plutôt que détectée : la détection
# ralentirait chaque morceau transcrit. work/langue.txt : choix pour la prochaine préparation
# (commun à tous les rushes) ; work/langue_rush.txt : langue du rush en cours (rangée avec lui).
LANGUAGES = {"fr": "Français", "en": "Anglais"}


def language(work_dir, name="langue.txt"):
    try:
        code = open(os.path.join(work_dir, name)).read().strip()
    except OSError:
        return "fr"
    return code if code in LANGUAGES else "fr"


def audio_streams(path):
    """Pistes son lisibles d'une vidéo : (indice parmi les pistes son, description), lues dans la
    sortie de « ffmpeg -i ». Une piste que ffmpeg ne sait pas décoder (son spatial « apac » des
    iPhone récents : « Audio: none ») est écartée."""
    import subprocess
    import imageio_ffmpeg
    out = subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-hide_banner", "-i", path], capture_output=True, text=True).stderr
    found = re.findall(r"Stream #\d+:\d+.*?: Audio: (.*)", out)
    return [(i, desc) for i, desc in enumerate(found) if not desc.startswith("none")]


def audio_map(path, label="son"):
    """Arguments ffmpeg (après « -i <path> », entrée 0) qui donnent le son du rush : la piste son
    seule, ou toutes ses pistes mélangées (un micro par personne enregistré sur des pistes
    séparées : enregistreur, micros sans fil, caméra à deux entrées). Sortie : [<label>].
    None si la vidéo n'a pas de son."""
    streams = audio_streams(path)
    if not streams:
        return None
    if len(streams) == 1:
        return ["-filter_complex", f"[0:a:{streams[0][0]}]anull[{label}]", "-map", f"[{label}]"]
    # Pistes additionnées (chacune garde son niveau), limiteur contre la saturation.
    inputs = "".join(f"[0:a:{i}]" for i, _ in streams)
    return ["-filter_complex", f"{inputs}amix=inputs={len(streams)}:duration=longest:normalize=0,"
                               f"alimiter=limit=0.97:level=0[{label}]", "-map", f"[{label}]"]


def edit_size(edit):
    """(largeur, hauteur) de la composition d'après edit.json (portrait sans indication)."""
    return int((edit or {}).get("width") or 1080), int((edit or {}).get("height") or 1920)


def gaps(segments, total):
    points = [0.0] + [x for s in segments for x in (s["start"], s["end"])] + [total]
    out = []
    for i in range(0, len(points) - 1, 2):
        a, b = points[i], points[i + 1]
        if b - a >= MIN_GAP:
            out.append({"id": GAP_BASE + len(out), "start": round(a, 3), "end": round(b, 3)})
    return out


def is_gap(moment_id):
    return GAP_BASE <= int(moment_id) < SPLIT_BASE


# --- Moments scindés ou fusionnés (page des moments) ---------------------------------
# work/segments.json (la transcription) ne change jamais. work/decoupage.json note :
#   {"splits": {"<segment>": [indices des mots où commence une nouvelle partie]},
#    "merges": [[id, id, …], …]}   (moments consécutifs fusionnés, ids effectifs)
# Une partie scindée a l'id part_id(segment, mot) ; un moment fusionné garde l'id du premier.
# Les ids ne dépendent que de ces réglages : annuler une scission ou une fusion rend les
# anciens ids, et les réglages de la page qui vont avec.
SPLIT_BASE = 1_000_000


def part_id(seg, word):
    return SPLIT_BASE + int(seg) * 1000 + int(word)


def load_decoupage(work_dir):
    try:
        d = json.load(open(os.path.join(work_dir, "decoupage.json")))
        return {"splits": {str(k): sorted({int(x) for x in v}) for k, v in (d.get("splits") or {}).items()},
                "merges": [[int(x) for x in g] for g in (d.get("merges") or []) if len(g) > 1]}
    except (OSError, ValueError, AttributeError, TypeError):
        return {"splits": {}, "merges": []}


def save_decoupage(work_dir, deco):
    deco = {"splits": {k: v for k, v in deco["splits"].items() if v}, "merges": [g for g in deco["merges"] if len(g) > 1]}
    json.dump(deco, open(os.path.join(work_dir, "decoupage.json"), "w"), indent=1)


def effective_segments(segments, deco):
    """Moments de parole après scissions et fusions, dans l'ordre du rush. Chaque mot garde son
    origine (« o » : segment, « oi » : rang du mot) ; « origins » : segments d'origine."""
    parts = []
    for s in segments:
        words = [dict(w, o=s["id"], oi=i) for i, w in enumerate(s["words"])]
        ks = [k for k in deco["splits"].get(str(s["id"]), []) if 0 < k < len(words)]
        bounds = [0] + ks + [len(words)]
        for j in range(len(bounds) - 1):
            ws = words[bounds[j]:bounds[j + 1]]
            first, last = j == 0, j == len(bounds) - 2
            parts.append({"id": s["id"] if first else part_id(s["id"], bounds[j]),
                          "start": s["start"] if first else ws[0]["a"],
                          "end": s["end"] if last else ws[-1]["b"],
                          "text": s["text"] if first and last else " ".join(w["w"] for w in ws),
                          "words": ws, "origins": [s["id"]],
                          **({} if first and last else {"splitFrom": s["id"]})})
    parts.sort(key=lambda p: p["start"])
    pos = {p["id"]: i for i, p in enumerate(parts)}
    for group in deco["merges"]:
        idx = sorted(pos[g] for g in group if g in pos)
        if len(idx) < 2 or idx != list(range(idx[0], idx[0] + len(idx))):
            continue  # moments disparus ou plus consécutifs : fusion ignorée
        members = [parts[i] for i in idx]
        merged = {"id": members[0]["id"], "start": members[0]["start"], "end": members[-1]["end"],
                  "text": " ".join(m["text"] for m in members),
                  "words": [w for m in members for w in m["words"]],
                  "origins": [o for m in members for o in m["origins"]],
                  "merged": [m["id"] for m in members]}
        for i in idx:
            parts[i] = None
        parts[idx[0]] = merged
    out = [p for p in parts if p]
    return out


def effective_gaps(segments, effective, total):
    """Plans sans parole : ceux de la transcription d'origine (ids stables), sauf ceux qu'une
    fusion a mis à l'intérieur d'un moment."""
    return [g for g in gaps(segments, total)
            if not any(e["start"] <= g["start"] and g["end"] <= e["end"] for e in effective)]


def word_map(effective):
    """(segment d'origine, rang du mot) -> (id effectif, rang dans ce moment)."""
    return {(w["o"], w["oi"]): (e["id"], i) for e in effective for i, w in enumerate(e["words"])}


def load_segments(work_dir):
    """(segments d'origine, moments effectifs, découpage)."""
    raw = json.load(open(os.path.join(work_dir, "segments.json")))
    deco = load_decoupage(work_dir)
    return raw, effective_segments(raw, deco), deco


def _words(text):
    return re.findall(r"[\w'’-]+", text.lower())


def is_retake(segments, i):
    """Vrai si la phrase du segment en position i est reprise dans l'un des 6 suivants."""
    wi = _words(segments[i]["text"])
    if len(wi) < 2:
        return False
    for j in range(i + 1, min(i + 7, len(segments))):
        wj = _words(segments[j]["text"])
        if wi[:3] == wj[:3] or difflib.SequenceMatcher(None, wi, wj[: len(wi) + 3]).ratio() > 0.6:
            return True
    return False


def suggested_keep(segments, work_dir):
    """Moments suggérés : work/suggestions.json (écrit par Claude) s'il existe, sinon
    tout sauf les 1res prises (phrase reprise juste après). Renvoie (ids, raisons)."""
    path = os.path.join(work_dir, "suggestions.json")
    if os.path.exists(path):
        suggest = json.load(open(path))
        keep, reasons = set(suggest["keep"]), {int(k): v for k, v in suggest.get("reasons", {}).items()}
        if any("origins" in s for s in segments):  # moments scindés ou fusionnés : ids d'origine
            ek, er = set(), {}
            for s in segments:
                origins = s.get("origins", [s["id"]])
                if any(o in keep for o in origins):
                    ek.add(s["id"])
                elif origins[0] in reasons:
                    er[s["id"]] = reasons[origins[0]]
            return ek, er
        return keep, reasons
    keep, reasons = set(), {}
    for pos, s in enumerate(segments):
        if is_retake(segments, pos):
            reasons[s["id"]] = "1re prise, reprise juste après"
        else:
            keep.add(s["id"])
    return keep, reasons


# Effets sonores fournis par défaut (public/sfx/<fichier>, synthétisés par make_sfx.py) :
# (identifiant, libellé, fichier, volume de base). « clavier » enchaîne key-1..4.wav.
SOUNDS = [
    ("cash", "Caisse", "cash.wav", 0.55),
    ("bop", "Bop", "bop.wav", 0.7),
    ("clavier", "Clavier", None, 0.45),
    ("whoosh", "Whoosh", "whoosh.wav", 0.6),
    ("pop", "Pop", "pop.wav", 0.6),
    ("ding", "Ding", "ding.wav", 0.5),
    ("boum", "Boum", "boum.wav", 0.75),
    ("montee", "Montée", "montee.wav", 0.5),
    ("glitch", "Glitch", "glitch.wav", 0.45),
    ("photo", "Déclencheur photo", "photo.wav", 0.6),
    ("faux", "Faux (buzzer)", "faux.wav", 0.45),
    ("juste", "Juste (carillon)", "juste.wav", 0.5),
]
# Effets visuels proposés sur chaque moment (page des moments, build_edit.py, render_hyperframes.py).
VISUALS = [
    ("typed", "Texte tapé"), ("chip", "Pastille"), ("zoomin", "Zoom avant"), ("zoomsec", "Zoom sec"),
    ("dezoom", "Dézoom"), ("shake", "Secousse"), ("flash", "Flash blanc"), ("highlight", "Mot mis en valeur"),
]


# Voix modifiées (effet « voix » d'un moment, champ « voice ») : identifiant, libellé, description.
# Filtres ffmpeg de chaque voix : voice_filter() (build_edit.py et tests/make_voice_demo.py) ;
# l'aperçu de la page des moments les imite avec Web Audio (selection/page.html, voiceChain).
VOICES = [
    ("tremblement", "Tremblement", "la voix tremble, avec un peu de robot (moment gêné ou drôle)"),
    ("robot", "Robot", "bourdonnement électronique et résonance métallique"),
    ("ecureuil", "Écureuil", "voix très aiguë, façon dessin animé"),
    ("lutin", "Lutin", "voix plus aiguë, légèrement doublée"),
    ("geant", "Géant", "voix grave, avec un peu d'écho"),
    ("megaphone", "Mégaphone", "voix serrée et saturée, comme dans un porte-voix"),
    ("telephone", "Téléphone", "voix étroite, légèrement numérique"),
    ("salle", "Grande salle", "écho de pièce vide"),
    ("batterie", "Batterie faible", "la voix ralentit et descend, comme une cassette qui s'épuise"),
    ("radio", "Vieille radio", "voix étroite, avec du souffle"),
    ("choeur", "Chœur", "la voix doublée, comme plusieurs personnes"),
]
VOICE_IDS = [v[0] for v in VOICES]
# Voix faites par HyperFrames au rendu et dans l'aperçu de la page (selection/hf_audio.js,
# même liste) ; les autres sont faites par ffmpeg dans build_edit.py (voice_filter).
HF_VOICES = {"megaphone", "telephone", "salle", "ecureuil", "lutin", "geant", "choeur"}


def hf_preview_files(out_dir):
    """Fichiers de l'aperçu des voix HyperFrames de la page des moments, copiés dans
    <out_dir>/hf/ (s'ils manquent ou ont changé) : selection/hf_audio.js, le lecteur et le
    moteur de HyperFrames, GSAP. Sans node_modules, la page garde l'imitation Web Audio."""
    import shutil
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    files = {"hf_audio.js": os.path.join(root, "selection", "hf_audio.js"),
             "hyperframes-player.global.js": os.path.join(root, "node_modules", "hyperframes", "dist", "hyperframes-player.global.js"),
             "hyperframe.runtime.iife.js": os.path.join(root, "node_modules", "hyperframes", "dist", "hyperframe.runtime.iife.js"),
             "gsap.min.js": os.path.join(root, "node_modules", "gsap", "dist", "gsap.min.js")}
    os.makedirs(os.path.join(out_dir, "hf"), exist_ok=True)
    for name, src in files.items():
        dst = os.path.join(out_dir, "hf", name)
        if os.path.exists(src) and (not os.path.exists(dst) or os.path.getmtime(dst) < os.path.getmtime(src)
                                    or os.path.getsize(dst) != os.path.getsize(src)):
            shutil.copy2(src, dst)


def voice_filter(voice, force=1.0):
    """Graphe ffmpeg (-filter_complex) d'une voix modifiée : entrée [0:a], sortie [r], même durée.
    Les voix de HF_VOICES ne servent plus qu'aux exemples de /rush/ (tests/make_voice_demo.py) :
    au rendu, ce sont celles de selection/hf_audio.js.
    Force 1 = réglage d'origine, jusqu'à 2. « batterie » : le ralentissement se fait avant, en
    Python (batterie_warp), ce filtre n'ajoute que le son fatigué."""
    f = max(0.0, min(2.0, float(force)))
    ring = lambda w: (f"asplit[d][m];sine=f=60:r=48000[s];[m][s]amultiply,volume=2.2[w];"
                      f"[d][w]amix=inputs=2:weights=1 {w:.3f}:normalize=0:duration=first")
    if voice == "robot":
        return f"[0:a]{ring(min(0.9, 0.55 * f))},aecho=0.8:0.8:7:{min(0.6, 0.35 * f):.3f}[r]"
    if voice == "ecureuil":
        return f"[0:a]rubberband=pitch={1 + 0.6 * f:.3f}:formant=shifted[r]"
    if voice == "lutin":
        return (f"[0:a]rubberband=pitch={1 + 0.3 * f:.3f}:formant=preserved,"
                "chorus=0.6:0.8:30|45:0.3|0.25:0.3|0.4:1.5|2[r]")
    if voice == "geant":
        return f"[0:a]rubberband=pitch={1 / (1 + 0.47 * f):.3f}:formant=shifted,aecho=0.8:0.6:60:0.2[r]"
    if voice == "megaphone":
        return (f"[0:a]highpass=f=600,lowpass=f=3200,volume={2 + 4 * f:.2f},asoftclip=type=atan,"
                "volume=0.5,aecho=0.8:0.4:12:0.25[r]")
    if voice == "telephone":
        return f"[0:a]highpass=f=400,lowpass=f=3000,acrusher=bits=9:mode=log:mix={min(1.0, 0.35 * f):.3f},volume=1.6[r]"
    if voice == "salle":
        d = [min(0.9, x * f) for x in (0.45, 0.3, 0.2)]
        return f"[0:a]aecho=0.8:0.7:90|170|260:{d[0]:.3f}|{d[1]:.3f}|{d[2]:.3f},lowpass=f=7000[r]"
    if voice == "batterie":
        return "[0:a]acrusher=bits=11:mode=log:mix=0.3,lowpass=f=5000[r]"
    if voice == "radio":
        return (f"[0:a]highpass=f=500,lowpass=f=2800,volume=2,asoftclip[v];"
                f"anoisesrc=c=brown:a={0.02 * f:.4f}:r=48000,highpass=f=1000[nz];"
                "[v][nz]amix=inputs=2:duration=first:normalize=0[r]")
    if voice == "choeur":
        dt = 0.015 * f
        return (f"[0:a]asplit=3[a][b][c];[b]rubberband=pitch={1 - dt:.4f},adelay=18[b2];"
                f"[c]rubberband=pitch={1 + dt:.4f},adelay=31[c2];"
                "[a][b2][c2]amix=inputs=3:normalize=0:duration=first,volume=0.6[r]")
    # Tremblement (par défaut) : vibrato et trémolo plus marqués qu'avant, avec un peu de robot.
    return (f"[0:a]vibrato=f=8:d={min(1.0, 0.8 * f):.3f},tremolo=f=10:d={min(1.0, 0.6 * f):.3f},"
            f"{ring(min(0.6, 0.3 * f))},aecho=0.8:0.8:7:{min(0.5, 0.25 * f):.3f}[r]")


def batterie_warp(x, force=1.0):
    """Batterie faible : lecture qui ralentit (hauteur et vitesse baissent ensemble, comme une
    cassette), sur un tableau numpy ; renvoie le signal ralenti (plus long), à remettre à la
    durée d'origine avec rubberband=tempo (hauteur gardée)."""
    import numpy as np
    n = len(x)
    if n < 2:
        return x
    t = np.arange(n) / n
    rate = 1.0 - min(0.8, 0.45 * max(0.0, float(force))) * t ** 1.6
    pos = np.cumsum(rate)
    pos = pos[pos < n - 1]
    y = np.interp(pos, np.arange(n), x)
    return y * (1 - 0.25 * np.arange(len(y)) / max(1, len(y)))


# Effets écartés de la page des moments (work/effets_page.json, « hidden ») : tous les effets
# fournis par défaut le sont tant que l'utilisateur ne les a pas cochés. « defauts » note la
# dernière liste appliquée : un fichier plus ancien reçoit les effets écartés depuis, en plus
# de ses propres choix (2 : sons par défaut ; 3 : voix modifiée, volume de la voix par moment
# et effets visuels).
DEFAULTS_VERSION = 3
DEFAULT_HIDDEN = {
    2: [i for i, _, _, _ in SOUNDS],
    3: ["voix", "gainvoix"] + [i for i, _ in VISUALS],
}


def hidden_effects(path):
    try:
        data = json.load(open(path))
        data = data if isinstance(data, dict) else {}
    except (OSError, ValueError):
        data = {}
    hidden = {h for h in data.get("hidden", []) if isinstance(h, str)}
    done = data.get("defauts") if isinstance(data.get("defauts"), int) else 0
    for version, ids in DEFAULT_HIDDEN.items():
        if done < version:
            hidden |= set(ids)
    return sorted(hidden)


def wav_len(path):
    """Durée d'un fichier WAV en secondes (0 s'il est illisible)."""
    try:
        with wave.open(path) as w:
            return round(w.getnframes() / w.getframerate(), 3)
    except (OSError, wave.Error, EOFError):
        return 0.0


SFX_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "public", "sfx")


def sound_catalog():
    """Catalogue des sons par défaut pour la page des moments (+ « voix », traitée à part), avec
    la durée de chaque son (« len », durée d'une répétition ; clavier : une frappe)."""
    out = [{"id": i, "label": label, "vol": vol, "len": wav_len(os.path.join(SFX_DIR, f)) if f else 0.08}
           for i, label, f, vol in SOUNDS]
    out.insert(3, {"id": "voix", "label": "Voix modifiée"})
    return out
