"""Construit le montage du rush en cours à partir de la transcription.

Entrées : work/segments.json (transcription par segment de parole),
          work/rush.txt (chemin du rush, écrit par prepare.sh) pour l'audio d'origine 48 kHz.
Sorties : src/data/edit.json  (clips, sous-titres, zooms, effets, bruitages)
          public/audio/voice.wav (voix montée, débruitée, avec les voix modifiées faites par ffmpeg ;
                                  celles de HF_VOICES, les graves, la clarté et le limiteur sont
                                  ajoutés au rendu par HyperFrames : selection/hf_audio.js)

Les décisions éditoriales (KEEP / FIXES / FUNNY / CASH / CHIPS / INTRO_TEXT) viennent de
work/edit_choices.json et désignent des segments de work/segments.json.
"""
import json
import os
import subprocess
import wave

import imageio_ffmpeg
import numpy as np

import fonts_lib
from moments_lib import (HF_VOICES, SOUNDS, VOICE_IDS, audio_map, batterie_warp, effective_gaps, frame_size, is_gap, load_segments, rush_duration,
                         suggested_keep, voice_filter, word_map)

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
WORK = os.path.join(ROOT, "work")
FPS = 30
FFMPEG = imageio_ffmpeg.get_ffmpeg_exe()

# Moments de parole : la transcription, avec les scissions et fusions faites sur la page
# (work/decoupage.json). SEG : id -> moment ; les choix ci-dessous (KEEP, FIXES…) désignent
# les segments d'origine et leurs mots, convertis plus bas (word_map).
raw_segments, segments, _deco = load_segments(WORK)
SEG = {s["id"]: s for s in segments}
WORD_MAP = word_map(segments)
# Plans sans parole (entre les segments), ajoutables depuis la page : moments_lib.is_gap.
GAPS = {g["id"]: g for g in effective_gaps(raw_segments, segments, rush_duration(WORK))}

# --- Dérush -----------------------------------------------------------------
# Choix de montage du rush en cours, lus dans work/edit_choices.json (format plus bas).
# KEEP : (segment, premier mot gardé, dernier mot gardé), None = jusqu'au bout.
# FIXES : corrections de transcription pour les sous-titres, (segment, index du mot) -> texte.
# FUNNY : moments « drôles » à voix modifiée (segment, premier mot, dernier mot[, voix]),
#         proposés sur la page comme effet « Voix modifiée » ; l'effet visuel sur le visage
#         est fait à la main au montage final.
# CASH : bruit de caisse enregistreuse sur un mot (segment, mot).
# CHIPS : éléments en surimpression avec son « bop » (segment, mot, texte).
# EXTRA_SFX : sons placés sur un mot (segment, mot, "<son>" ou "perso-<nom>").
# EXTRA_VFX : effets visuels placés par Claude, {segment: [effets]}.
# INTRO_TEXT : intro tapée au clavier, pendant la première phrase.
KEEP, FIXES, FUNNY, CASH, CHIPS, EXTRA_SFX, EXTRA_VFX, INTRO_TEXT = [], {}, [], [], [], [], {}, ""

# Sans edit_choices.json pour ce rush (préparé depuis l'interface, sans Claude) : montage
# générique, moments suggérés, sans effets ni intro.
EDIT_RUSH = None
CURRENT_RUSH = os.path.basename(open(os.path.join(WORK, "rush.txt")).read().strip())

# Choix écrits pour le rush en cours dans work/edit_choices.json (par Claude, d'après les
# consignes de montage) : ils remplacent ceux ci-dessus et survivent aux mises à jour.
#   {"rush": "MonRush.mov", "keep": [[segment, premier mot, dernier mot ou null], …],
#    "fixes": [[segment, mot, "texte"], …], "funny": [[segment, premier, dernier, "voix"?], …],
#            (voix modifiée : moments_lib.VOICES, « tremblement » par défaut, ou robot, ecureuil,
#            lutin, geant, megaphone, telephone, salle, batterie, radio, choeur)
#    "cash": [[segment, mot], …], "chips": [[segment, mot, "texte"], …], "intro": "texte",
#    "sfx": [[segment, mot, "<son>"], …],  (sons par défaut de moments_lib.SOUNDS : whoosh, pop,
#            ding, boum, montee, glitch, photo, faux, juste… ou personnels : "perso-<nom>")
#    "vfx": {"segment": [effet visuel, …]}}  (comme sur la page, temps depuis le début de
#            l'extrait du moment : {"kind": "zoomin"|"zoomsec"|"dezoom"|"shake", "at", "dur",
#            "force" (%)}, {"kind": "flash", "at", "dur"}, {"kind": "highlight", "text": "mot",
#            "color": "#rrggbb"})
CHOICES_PATH = os.path.join(WORK, "edit_choices.json")
if os.path.exists(CHOICES_PATH):
    _c = json.load(open(CHOICES_PATH))
    if _c.get("rush") == CURRENT_RUSH:
        EDIT_RUSH = CURRENT_RUSH
        if _c.get("keep"):
            KEEP = [(int(k[0]), int(k[1]), None if k[2] is None else int(k[2])) for k in _c["keep"]]
        FIXES = {(int(f[0]), int(f[1])): str(f[2]) for f in _c.get("fixes", [])}
        FUNNY = [(int(f[0]), int(f[1]), int(f[2])) + ((str(f[3]),) if len(f) > 3 and str(f[3]) in VOICE_IDS else ())
                 for f in _c.get("funny", [])]
        CASH = [(int(c[0]), int(c[1])) for c in _c.get("cash", [])]
        CHIPS = [(int(c[0]), int(c[1]), str(c[2])) for c in _c.get("chips", [])]
        EXTRA_SFX = [(int(x[0]), int(x[1]), str(x[2])) for x in _c.get("sfx", [])]
        EXTRA_VFX = {int(k): [v for v in vs if isinstance(v, dict)] for k, vs in (_c.get("vfx") or {}).items()}
        INTRO_TEXT = str(_c.get("intro") or "")
        if not _c.get("keep"):  # sans liste : les moments suggérés
            _keep_ids, _ = suggested_keep(segments, WORK)
            KEEP = [(sid, 0, None) for sid in sorted(_keep_ids)]
if CURRENT_RUSH == EDIT_RUSH and (_deco["splits"] or _deco["merges"]):
    # Choix écrits pour les segments d'origine : vers les moments scindés ou fusionnés.
    def _at(seg, i):
        return WORD_MAP.get((seg, i))

    def _keep_range(seg, first, last):
        n = len(raw_segments[seg]["words"]) if 0 <= seg < len(raw_segments) else 0
        last = n - 1 if last is None else last
        spans = {}
        for i in range(max(0, first), min(n, last + 1)):
            hit = _at(seg, i)
            if hit:
                spans.setdefault(hit[0], []).append(hit[1])
        return [(eid, min(ix), None if max(ix) == len(SEG[eid]["words"]) - 1 else max(ix)) for eid, ix in spans.items()]
    KEEP = [k for seg, a, b in KEEP for k in _keep_range(seg, a, b)]
    FIXES = {_at(*k): v for k, v in FIXES.items() if _at(*k)}
    FUNNY = [(_at(f[0], f[1])[0], _at(f[0], f[1])[1], _at(f[0], f[2])[1]) + tuple(f[3:])
             for f in FUNNY if _at(f[0], f[1]) and _at(f[0], f[2]) and _at(f[0], f[1])[0] == _at(f[0], f[2])[0]]
    CASH = [_at(*c) for c in CASH if _at(*c)]
    CHIPS = [_at(c[0], c[1]) + (c[2],) for c in CHIPS if _at(c[0], c[1])]
    EXTRA_SFX = [_at(x[0], x[1]) + (x[2],) for x in EXTRA_SFX if _at(x[0], x[1])]
    # Effets visuels de Claude (temps depuis le début de l'extrait du segment) : sur le moment
    # qui contient le 1er mot du segment, décalés du début de ce moment.
    _vfx = {}
    for seg, fxs in EXTRA_VFX.items():
        hit = _at(seg, 0)
        if hit:
            shift = raw_segments[seg]["start"] - SEG[hit[0]]["start"]
            _vfx.setdefault(hit[0], []).extend(
                {**v, "at": round(float(v.get("at", 0)) + shift, 3)} if "at" in v else v for v in fxs)
    EXTRA_VFX = _vfx
if CURRENT_RUSH != EDIT_RUSH:
    if not segments:
        raise SystemExit("Aucune parole détectée dans le rush : rien à monter.")
    keep_ids, _ = suggested_keep(segments, WORK)
    KEEP = [(s["id"], 0, None) for s in segments if s["id"] in keep_ids] or [(s["id"], 0, None) for s in segments]
    FIXES, FUNNY, CASH, CHIPS, INTRO_TEXT, EXTRA_SFX, EXTRA_VFX = {}, [], [], [], "", [], {}

# --- Sélection faite sur la page des moments ---------------------------------------
# Si work/selection.json existe ({"keep": [ids de segments]}, écrit par Claude depuis la
# page), elle remplace KEEP : segments entiers dans l'ordre du rush (découpes de la page
# comprises, pas les coupes au mot de KEEP). Les effets qui visent un segment retiré sont ignorés.
# Habillage par défaut de chaque moment, pour l'aperçu des moments sur la page (avant
# que la sélection ne filtre ces listes).
OVERLAY_DEFAULTS = {
    "fixes": {},
    "cashWord": {str(seg): i for seg, i in CASH},
    "chips": {str(seg): {"word": i, "text": label} for seg, i, label in CHIPS},
    "intro": INTRO_TEXT,
    "introAt": 0.2,  # départ de l'intro dans l'extrait du 1er moment gardé
    "introType": 1.6,  # durée de frappe
    "vfx": {str(seg): v for seg, v in EXTRA_VFX.items()},  # effets visuels placés par Claude
}
for (seg, i), text in FIXES.items():
    OVERLAY_DEFAULTS["fixes"].setdefault(str(seg), {})[str(i)] = text



def page_start(seg_id):
    """Début de l'extrait d'un moment sur la page (les temps de la page en partent)."""
    if is_gap(seg_id):  # l'extrait d'un plan sans parole commence au début du plan
        return GAPS[seg_id]["start"]
    return max(0.0, SEG[seg_id]["start"] - 0.05)


def funny_default(seg_id, a, b, voice="tremblement"):
    """Fenêtre de voix modifiée d'un moment drôle, en temps de la page."""
    words = SEG[seg_id]["words"]
    at = words[a]["a"] - 0.05 - page_start(seg_id)
    out = {"kind": "voix", "at": round(max(0.0, at), 2), "dur": round(words[b]["b"] - words[a]["a"] + 0.15, 2)}
    return out | ({"voice": voice} if voice != "tremblement" else {})


# Effets sonores proposés par défaut sur la page, par moment (repris de CASH et FUNNY :
# « voix » = voix modifiée, qui tremble).
SFX_DEFAULTS = {}
for seg_id, _ in CASH:
    SFX_DEFAULTS.setdefault(seg_id, ["cash"])
for seg_id, i, kind in EXTRA_SFX:  # sons personnels : sur leur mot, réglables sur la page
    if 0 <= i < len(SEG[seg_id]["words"]):
        at = SEG[seg_id]["words"][i]["a"] - page_start(seg_id)
        SFX_DEFAULTS.setdefault(seg_id, []).append({"kind": kind, "at": round(at, 3), "repeat": 1, "gap": 0.1})
VOICE_DEFAULTS = {f[0]: funny_default(*f) for f in FUNNY}
for seg_id, v in VOICE_DEFAULTS.items():
    SFX_DEFAULTS.setdefault(seg_id, []).append(v)
# Découpes faites sur la page : segment -> instants de coupe (s depuis le début de l'extrait),
# et parties gardées : segment -> {indices} (None = tout le segment).
CUTS, PARTS = {}, {}
# Moments dont les effets ont été enregistrés avant l'effet « voix » : défaut de FUNNY gardé.
SFX_LEGACY = set()
# Effets choisis sur la page : segment -> liste d'effets. Chaque effet est soit un nom
# ("cash", "bop", "clavier" : réglages par défaut), soit un réglage précis
# {"kind", "at": départ en s depuis le début de l'extrait du moment, "repeat", "gap", "dur"}.
PAGE_SFX = {}
# Effets visuels choisis sur la page : segment -> [{"kind": "typed" | "chip", "text", "at",
# "type" (durée de frappe, texte tapé), "dur" (durée d'affichage), "sound": bool}].
# Sans choix sur la page : intro tapée sur le 1er moment gardé, pastilles CHIPS.
PAGE_VFX = {}
PAGE_GAINS = {}  # volume de la voix par moment (page des moments)

selection_path = os.path.join(WORK, "selection.json")
if os.path.exists(selection_path):
    selection = json.load(open(selection_path))
    # « keep » : ids de moments, ou « id/k » pour la partie k d'un moment découpé.
    for item in selection["keep"]:
        if isinstance(item, str) and "/" in item:
            mid, k = (int(x) for x in item.split("/"))
            if PARTS.get(mid, set()) is not None:
                PARTS.setdefault(mid, set()).add(k)
        else:
            PARTS[int(item)] = None
    CUTS = {int(k): sorted(float(x) for x in v) for k, v in selection.get("cuts", {}).items()
            if v and not is_gap(int(k))}
    SFX_LEGACY = {int(k) for k in selection.get("sfx_legacy", [])}
    PAGE_GAINS = {int(k): float(v) for k, v in selection.get("gains", {}).items()}
    chosen = sorted(PARTS)
    page_trims = {int(k): v for k, v in selection.get("trims", {}).items()}
    KEEP = []
    for mid in chosen:
        if not is_gap(mid) and mid not in SEG:
            continue  # moment qui n'existe plus (scindé ou fusionné depuis)
        if not is_gap(mid):
            # Moment entier, comme sur la page : les coupes au mot de KEEP ne valent que pour le
            # montage par défaut ; sur la page, on coupe avec « Couper ici ».
            KEEP.append((mid, 0, None))
        elif mid in GAPS:
            # Plan sans parole : début et fin choisis sur la page, en s depuis le début du plan.
            g = GAPS[mid]
            length = g["end"] - g["start"]
            t = page_trims.get(mid, {})
            t_in = min(max(0.0, float(t.get("in", 0))), length - 0.2)
            t_out = min(max(t_in + 0.2, float(t.get("out", length))), length)
            KEEP.append((mid, t_in, t_out))
    # Dans l'ordre du rush, plans sans parole compris.
    KEEP.sort(key=lambda k: GAPS[k[0]]["start"] + k[1] if is_gap(k[0]) else SEG[k[0]]["start"])
    kept_ids = {k[0] for k in KEEP}
    FUNNY = [f for f in FUNNY if f[0] in kept_ids]
    CHIPS = [c for c in CHIPS if c[0] in kept_ids]

    # Textes corrigés sur la page : les mots corrigés reprennent les horodatages des mots
    # d'origine (alignement mot à mot), et remplacent les FIXES de ce segment.
    def realign(words, corrected):
        import difflib
        import re

        norm = lambda w: re.sub(r"[^\w]", "", w.lower())
        old = [w["w"] for w in words]
        new = corrected.split()
        texts = [""] * len(old)
        ops = difflib.SequenceMatcher(None, [norm(w) for w in old], [norm(w) for w in new]).get_opcodes()
        for tag, i1, i2, j1, j2 in ops:
            if i1 == i2:  # mots ajoutés : collés au mot précédent (ou au suivant en tête)
                k = max(0, i1 - 1) if i1 > 0 else 0
                extra = " ".join(new[j1:j2])
                texts[k] = f"{texts[k]} {extra}".strip() if i1 > 0 else f"{extra} {texts[k]}".strip()
                continue
            chunk = new[j1:j2]
            for n, w in enumerate(chunk):  # répartis sur la durée des mots d'origine
                k = i1 + n * (i2 - i1) // len(chunk)
                texts[k] = f"{texts[k]} {w}".strip()
        return texts

    for seg_key, text in selection.get("corrections", {}).items():
        seg_id = int(seg_key)
        if seg_id not in kept_ids:
            continue
        # Texte vide : sous-titres supprimés pour ce moment (mots gardés, sans texte affiché).
        texts = realign(SEG[seg_id]["words"], text) if text.strip() else [""] * len(SEG[seg_id]["words"])
        for w, t in zip(SEG[seg_id]["words"], texts):
            w["w"] = t
        FIXES = {k: v for k, v in FIXES.items() if k[0] != seg_id}

    PAGE_SFX = {int(k): v for k, v in selection.get("sfx", {}).items() if int(k) in kept_ids}
    PAGE_VFX = {int(k): v for k, v in selection.get("vfx", {}).items() if int(k) in kept_ids}
    # Un moment dont les effets ont été choisis sur la page ne garde la caisse
    # (placée sur le bon mot) que si elle est toujours cochée.
    # La caisse par défaut reste sur son mot tant qu'elle est cochée sans réglage précis.
    CASH = [c for c in CASH if c[0] in kept_ids and (c[0] not in PAGE_SFX or "cash" in PAGE_SFX[c[0]])]
    # Sons personnels de Claude : remplacés par les effets choisis sur la page pour ce moment.
    EXTRA_SFX = [x for x in EXTRA_SFX if x[0] in kept_ids and x[0] not in PAGE_SFX]
    print(f"Sélection de la page : {len(KEEP)} segments, "
          f"{len(selection.get('corrections', {}))} textes corrigés, {len(PAGE_SFX)} moments avec effets choisis")

# --- Audio d'origine ----------------------------------------------------------
SR = 48000
# Piste son du rush, ou toutes ses pistes mélangées (moments_lib.audio_map, comme prepare.sh).
_rush_path = os.path.join(ROOT, open(os.path.join(WORK, "rush.txt")).read().strip())
raw = subprocess.run(
    [FFMPEG, "-v", "error", "-i", _rush_path, *(audio_map(_rush_path) or ["-map", "0:a:0"]),
     "-ac", "1", "-ar", str(SR), "-f", "s16le", "-"],
    check=True, capture_output=True,
).stdout
audio = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768


def refine(t, window=0.12):
    """Recale une coupe au point le plus silencieux autour de t (trames de 10 ms)."""
    hop = int(SR * 0.01)
    lo, hi = int((t - window) * SR), int((t + window) * SR)
    best, best_e = t, 1e9
    for s in range(max(0, lo), min(len(audio) - hop, hi), hop):
        e = float(np.mean(audio[s:s + hop] ** 2))
        if e < best_e:
            best, best_e = s / SR, e
    return best


# --- Clips --------------------------------------------------------------------
clips = []
out_frame = 0
word_events = []  # mots avec leur temps de sortie
for seg_id, first, last in KEEP:
    if is_gap(seg_id):  # plan sans parole : pas de mots, pas de sous-titre
        g = GAPS[seg_id]
        f_in = round((g["start"] + first) * FPS)
        dur = max(1, round((g["start"] + last) * FPS) - f_in)
        clips.append({"seg": seg_id, "trimBefore": f_in, "durationInFrames": dur, "from": out_frame,
                      "text": "Sans parole"})
        out_frame += dur
        continue
    seg = SEG[seg_id]
    words = seg["words"]
    last_i = len(words) - 1 if last is None else last
    start = seg["start"] if first == 0 else refine(words[first]["a"])
    end = seg["end"] if last is None else refine(words[last_i]["b"])
    # Découpe faite sur la page : parties [0, coupe 1], [coupe 1, coupe 2]… de l'extrait,
    # coupes à l'image près où elles ont été placées (curseur de la page, image par image) ;
    # parties gardées contiguës fusionnées.
    spans = [(start, end, None)]  # (début, fin, parties couvertes ; None = tout)
    bounds = None
    if seg_id in CUTS:
        bounds = [-1e9] + [round((page_start(seg_id) + c) * FPS) / FPS for c in CUTS[seg_id]] + [1e9]
        kept_parts = PARTS.get(seg_id)
        spans = []
        for k in range(len(bounds) - 1):
            a, b = max(start, bounds[k]), min(end, bounds[k + 1])
            if b - a < 0.1 or (kept_parts is not None and k not in kept_parts):
                continue
            if spans and abs(spans[-1][1] - a) < 1e-6:
                spans[-1] = (spans[-1][0], b, spans[-1][2] | {k})
            else:
                spans.append((a, b, {k}))
    for span_start, span_end, span_parts in spans:
        f_in = round(span_start * FPS)
        f_out = round(span_end * FPS)
        dur = f_out - f_in
        clips.append({"seg": seg_id, "trimBefore": f_in, "durationInFrames": dur, "from": out_frame, "text": ""})
        n_before = len(word_events)
        for i in range(first, last_i + 1):
            w = words[i]
            if span_parts is not None:  # moment découpé : mot rangé dans sa partie par son milieu
                mid = (w["a"] + w["b"]) / 2
                if next(k for k in range(len(bounds) - 1) if mid < bounds[k + 1]) not in span_parts:
                    continue
            text = FIXES.get((seg_id, i), w["w"])
            a = min(max(w["a"], f_in / FPS), f_out / FPS)
            b = min(max(w["b"], a + 0.05), f_out / FPS)
            word_events.append({
                "seg": seg_id, "i": i, "text": text,
                "start": out_frame / FPS + (a - f_in / FPS),
                "end": out_frame / FPS + (b - f_in / FPS),
            })
        clips[-1]["text"] = " ".join(x["text"] for x in word_events[n_before:] if x["text"])
        out_frame += dur

total_frames = out_frame

# --- Source 4K du rendu final --------------------------------------------------------
# Seules les images gardées (plus une marge) sont encodées en 4K par make_hd.py, bout à
# bout : chaque clip y a donc sa propre position (trimBeforeHd).
# Les plages qui se chevauchent sont fusionnées (le fichier 4K suit l'ordre du rush et
# ne contient chaque image qu'une fois).
HD_PAD = 6
hd_ranges = []
for a, b in sorted(
    (max(0, c["trimBefore"] - HD_PAD), c["trimBefore"] + c["durationInFrames"] + HD_PAD) for c in clips
):
    if hd_ranges and a <= hd_ranges[-1][1]:
        hd_ranges[-1][1] = max(hd_ranges[-1][1], b)
    else:
        hd_ranges.append([a, b])
for c in clips:
    offset = 0
    for a, b in hd_ranges:
        if a <= c["trimBefore"] < b:
            c["trimBeforeHd"] = offset + c["trimBefore"] - a
            break
        offset += b - a


def word_time(seg_id, i, key="start"):
    """Temps d'un mot dans le montage, None s'il a été coupé."""
    for w in word_events:
        if w["seg"] == seg_id and w["i"] == i:
            return w[key]
    return None


# --- Sous-titres ----------------------------------------------------------------
# Jusqu'à 6 mots (26 lettres), sur 2 lignes de même taille et de largeurs aussi proches que
# possible : la coupure entre les lignes est choisie au rendu, en mesurant le texte dans la
# police choisie (render_lib.caption_layout) ; « top » / « bottom » en sont une estimation au nombre de
# lettres. Masqués à l'écran pendant un texte tapé (l'intro). Même règle dans la page des
# moments (captionGroups).
cap_words = [w for w in word_events if w["text"]]
captions = []
group = []


def balanced_split(words):
    """Coupure en 2 lignes de longueurs les plus proches (en lettres) ; 0 pour un seul mot."""
    if len(words) < 2:
        return 0
    lens = [len(w) for w in words]
    total = sum(lens) + len(words) - 1
    best, best_k = None, 1
    for k in range(1, len(words)):
        top = sum(lens[:k]) + k - 1
        diff = abs(top - (total - top - 1))
        if best is None or diff < best:
            best, best_k = diff, k
    return best_k


def flush():
    if not group:
        return
    words = [g["text"] for g in group]
    split = balanced_split(words)
    captions.append({
        "top": " ".join(words[:split]),
        "bottom": " ".join(words[split:]),
        "words": words,
        "times": [round(g["start"], 3) for g in group],  # début de chaque mot (mot prononcé en couleur)
        "start": group[0]["start"],
        "end": group[-1]["end"],
    })
    group.clear()


for w in cap_words:
    ends_sentence = w["text"].rstrip().endswith((".", "!", "?"))
    ends_clause = w["text"].rstrip().endswith(",")
    gap = group and w["start"] - group[-1]["end"] > 0.35
    if gap:
        flush()
    group.append(w)
    chars = sum(len(g["text"]) for g in group)
    if ends_sentence or len(group) >= 6 or chars > 26 or (ends_clause and len(group) >= 3):
        flush()
flush()
# Chaque sous-titre reste jusqu'au suivant (pas de trou à l'écran).
# Sans déborder sur un plan sans parole qui suit.
for a, b in zip(captions, captions[1:]):
    a["end"] = b["start"]
if captions:  # aucun sous-titre possible : moments sans sous-titres, plans sans parole
    captions[-1]["end"] = total_frames / FPS
for cap in captions:
    for c in clips:
        c_start, c_end = c["from"] / FPS, (c["from"] + c["durationInFrames"]) / FPS
        if c_start <= cap["start"] < c_end:
            nxt = clips.index(c) + 1
            if nxt < len(clips) and is_gap(clips[nxt]["seg"]):
                cap["end"] = min(cap["end"], c_end)
            break

# --- Zooms ----------------------------------------------------------------------
# Alternance zoom sec (punch) / zoom in (progressif), plan changé toutes les ~1,8 s
# sur une frontière de mot, pour que l'image ne soit jamais statique.
zooms = []
t = 0.0
kind = 0
boundaries = sorted({round(w["start"], 3) for w in word_events} | {c["from"] / FPS for c in clips})
while t < total_frames / FPS - 0.3:
    target = t + 1.8
    nxt = min((b for b in boundaries if b >= t + 1.3), key=lambda b: abs(b - target), default=total_frames / FPS)
    nxt = min(nxt, total_frames / FPS)
    zooms.append({"start": round(t, 3), "end": round(nxt, 3), "type": "in" if kind % 2 == 0 else "punch"})
    kind += 1
    t = nxt
zooms[-1]["end"] = total_frames / FPS

# --- Temps de la page -> temps du montage -------------------------------------------
# Sur la page, les temps d'un moment sont comptés depuis le début de son extrait
# (0,05 s avant le segment de parole).
# Un moment découpé a plusieurs clips.
clips_of = {}
for c in clips:
    clips_of.setdefault(c["seg"], []).append(c)
KEEP = [k for k in KEEP if k[0] in clips_of]  # moment dont aucune partie n'est gardée


def clip_at(seg_id, at):
    """Clip du moment qui contient l'instant « at » de l'extrait (ou le plus proche)."""
    t = page_start(seg_id) + at
    return min(clips_of[seg_id], key=lambda c: max(c["trimBefore"] / FPS - t,
                                                   t - (c["trimBefore"] + c["durationInFrames"]) / FPS, 0))


def page_time(seg_id, at):
    """Temps dans le montage d'un instant « at » de l'extrait du moment (borné au clip)."""
    c = clip_at(seg_id, at)
    t = c["from"] / FPS + page_start(seg_id) + at - c["trimBefore"] / FPS
    return min(max(t, c["from"] / FPS), (c["from"] + c["durationInFrames"] - 1) / FPS)


def clip_end(c):
    return (c["from"] + c["durationInFrames"]) / FPS


# --- Voix modifiée (qui tremble) -----------------------------------------------------
# Par défaut sur les mots de FUNNY ; sur un moment dont les effets ont été choisis sur la
# page, seulement si « voix » y est cochée (départ et durée en temps de la page).
funny = []


def add_voice(seg_id, at, dur, force=1.0, voice="tremblement"):
    s = page_time(seg_id, at)
    e = min(s + max(0.05, dur), clip_end(clip_at(seg_id, at + dur)), total_frames / FPS)
    force = min(2.0, max(0.0, force))
    if e > s and force > 0:
        funny.append({"start": round(s, 3), "end": round(e, 3), "force": round(force, 2),
                      "voice": voice if voice in VOICE_IDS else "tremblement"})


for seg_id, _, _ in KEEP:
    default = VOICE_DEFAULTS.get(seg_id)
    if seg_id not in PAGE_SFX or seg_id in SFX_LEGACY:
        if default:
            add_voice(seg_id, default["at"], default["dur"], voice=default.get("voice", "tremblement"))
        continue
    for fx in PAGE_SFX[seg_id]:
        if fx == "voix":
            v = default or {"at": 0.0, "dur": sum(c["durationInFrames"] for c in clips_of[seg_id]) / FPS}
            add_voice(seg_id, v["at"], v["dur"], voice=v.get("voice", "tremblement"))
        elif isinstance(fx, dict) and fx.get("kind") == "voix":
            force = fx.get("force")
            add_voice(seg_id, float(fx.get("at", 0)), float(fx.get("dur") or 1.0),
                      1.0 if force is None else float(force), str(fx.get("voice") or "tremblement"))
funny.sort(key=lambda f: f["start"])


# --- Bruitages ------------------------------------------------------------------
sfx = []
for seg_id, i in CASH:
    if word_time(seg_id, i) is not None:  # mot pas coupé
        sfx.append({"src": "sfx/cash.wav", "start": round(word_time(seg_id, i), 3), "volume": 0.55})
# Effets ajoutés sur la page. Sans réglage précis : au début du moment (la caisse par défaut
# est déjà placée sur son mot ci-dessus). Avec réglage : « at » est compté depuis le début de
# l'extrait du moment sur la page (0,05 s avant le segment), comme dans l'aperçu.
# Sons par défaut (moments_lib.SOUNDS) : un déclenchement, sauf le clavier (6 frappes).
SFX_FILES = {i: (f"sfx/{f}", vol) for i, _, f, vol in SOUNDS if f}
SFX_PRESETS = {i: (1, 0.1) for i, _, _, _ in SOUNDS} | {"clavier": (6, 0.08)}
# Sons personnels (public/sfx/perso/<nom>.wav) : un déclenchement par défaut, comme le bop.
for _n in sorted(os.listdir(os.path.join(ROOT, "public", "sfx", "perso"))) if os.path.isdir(os.path.join(ROOT, "public", "sfx", "perso")) else []:
    if _n.endswith(".wav"):
        SFX_FILES[f"perso-{_n[:-4]}"] = (f"sfx/perso/{_n}", 0.6)
        SFX_PRESETS[f"perso-{_n[:-4]}"] = (1, 0.1)
for seg_id, i, kind in EXTRA_SFX:
    if kind in SFX_FILES and word_time(seg_id, i) is not None:
        src, vol = SFX_FILES[kind]
        sfx.append({"src": src, "start": round(word_time(seg_id, i), 3), "volume": vol})
for seg_id, effects in PAGE_SFX.items():
    if seg_id not in clips_of:
        continue
    # Sans réglage : au premier mot gardé (début du clip pour un plan sans parole).
    t0 = next((w["start"] for w in word_events if w["seg"] == seg_id), clips_of[seg_id][0]["from"] / FPS)
    for fx in effects:
        if fx == "voix" or (isinstance(fx, dict) and fx.get("kind") == "voix"):
            continue  # voix modifiée : traitée sur la piste voix
        if isinstance(fx, str):
            if fx == "cash" and any(c[0] == seg_id for c in CASH):
                continue
            if fx.startswith("perso-") and fx not in SFX_FILES:
                continue  # son personnel retiré de la bibliothèque
            kind, start, (repeat, gap), dur = fx, t0, SFX_PRESETS.get(fx, (1, 0.1)), None
        elif isinstance(fx, dict) and fx.get("kind") in SFX_PRESETS:
            kind = fx["kind"]
            start = page_time(seg_id, float(fx.get("at", 0)))
            repeat = max(1, min(20, int(fx.get("repeat", SFX_PRESETS[kind][0]))))
            gap = max(0.02, float(fx.get("gap", SFX_PRESETS[kind][1])))
            dur = float(fx["dur"]) if fx.get("dur") else None
            gain = max(0.0, min(2.0, float(fx.get("vol", 1))))  # volume réglé sur la page (1 = 100 %)
        else:
            continue
        if isinstance(fx, str):
            gain = 1.0
        for k in range(repeat):
            src, vol = SFX_FILES.get(kind, (f"sfx/key-{k % 4 + 1}.wav", 0.45))
            event = {"src": src, "start": round(start + gap * k, 3), "volume": round(vol * gain, 3)}
            if dur:
                event["dur"] = round(dur, 3)
            sfx.append(event)
# --- Effets visuels : textes tapés et pastilles ------------------------------------
chip_defaults = {seg: (i, label) for seg, i, label in CHIPS}


def vfx_defaults(seg_id):
    """Effets visuels par défaut d'un moment, en temps de la page."""
    out = []
    if INTRO_TEXT and seg_id == KEEP[0][0]:  # intro tapée sur le 1er moment gardé
        if is_gap(seg_id):
            clip_len = KEEP[0][2]  # fin choisie du plan sans parole
        else:
            clip_len = SEG[seg_id]["end"] - page_start(seg_id)  # jusqu'à la fin de la parole
        at = OVERLAY_DEFAULTS["introAt"]
        out.append({"kind": "typed", "text": INTRO_TEXT, "at": at, "type": OVERLAY_DEFAULTS["introType"],
                    "dur": round(clip_len - at, 2), "sound": True})
    out += EXTRA_VFX.get(seg_id, [])
    if seg_id in chip_defaults:
        i, label = chip_defaults[seg_id]
        if word_time(seg_id, i) is None:  # mot de la pastille coupé
            return out
        at = SEG[seg_id]["words"][i]["a"] - page_start(seg_id)
        out.append({"kind": "chip", "text": label, "at": round(at, 3), "dur": 2.2, "sound": True})
    return out


typed, chips = [], []
# Effets visuels fournis par défaut (moments_lib.VISUALS) : zooms, secousses, flashs, mots mis
# en valeur. « force » en % (zoom : grossissement ; secousse : amplitude).
zoom_fx, shakes, flashes, highlights = [], [], [], []
for seg_id, _, _ in KEEP:
    effects = PAGE_VFX.get(seg_id, vfx_defaults(seg_id))
    for fx in effects:
        if not isinstance(fx, dict):
            continue
        kind = fx.get("kind")
        if kind in ("zoomin", "zoomsec", "dezoom", "shake", "flash"):
            at = float(fx.get("at", 0))
            start = page_time(seg_id, at)
            dur = max(0.05, float(fx.get("dur", 0.8)))
            force = max(0.0, min(150.0, float(fx.get("force", 25)))) / 100
            if kind == "zoomin":  # zoom progressif, gardé jusqu'à la fin de la partie du moment
                zoom_fx.append({"type": "in", "start": round(start, 3), "anim": round(dur, 3),
                                "end": round(clip_end(clip_at(seg_id, at)), 3), "force": force})
            elif kind in ("zoomsec", "dezoom"):
                zoom_fx.append({"type": "punch" if kind == "zoomsec" else "out", "start": round(start, 3),
                                "anim": round(dur, 3), "end": round(start + dur, 3), "force": force})
            elif kind == "shake":
                shakes.append({"start": round(start, 3), "end": round(start + dur, 3), "force": force})
            else:
                flashes.append({"start": round(start, 3), "end": round(start + dur, 3)})
            continue
        if kind == "highlight":  # mot (ou mots) du sous-titre en couleur, sur tout le moment
            word = str(fx.get("text", "")).strip()
            if word:
                color = fx.get("color") if isinstance(fx.get("color"), str) and len(fx["color"]) == 7 else "#ffd84d"
                highlights.append({"words": word, "color": color,
                                   "start": round(clips_of[seg_id][0]["from"] / FPS, 3),
                                   "end": round(clip_end(clips_of[seg_id][-1]), 3)})
            continue
        if not str(fx.get("text", "")).strip():
            continue
        start = page_time(seg_id, float(fx.get("at", 0)))
        end = start + max(0.1, float(fx.get("dur", 2.2)))  # peut déborder sur le moment suivant
        if fx.get("kind") == "typed":
            # un texte tapé reste dans son moment (dans sa partie s'il est découpé)
            end = min(end, clip_end(clip_at(seg_id, float(fx.get("at", 0)))))
            type_dur = max(0.1, float(fx.get("type", 1.6)))
            typed.append({"text": fx["text"], "start": round(start, 3), "typeDuration": type_dur, "end": round(end, 3)})
            if fx.get("sound", True):  # une frappe toutes les 2 lettres
                n_chars = max(1, len(fx["text"].replace("\n", "")))
                for k in range(0, n_chars, 2):
                    sfx.append({"src": f"sfx/key-{k // 2 % 4 + 1}.wav",
                                "start": round(start + type_dur * k / n_chars, 3), "volume": 0.45})
        elif fx.get("kind") == "chip":
            chips.append({"text": fx["text"], "start": round(start, 3), "end": round(end, 3)})
            if fx.get("sound", True):
                sfx.append({"src": "sfx/bop.wav", "start": round(start, 3), "volume": 0.7})

# Volume de tous les effets sonores (page des moments, work/effets_page.json « volume »,
# 1 = 100 %), plafonné au volume maximal d'un son.
try:
    SFX_GAIN = max(0.0, min(2.0, float(json.load(open(os.path.join(WORK, "effets_page.json"))).get("volume", 1))))
except (OSError, ValueError, TypeError, AttributeError):
    SFX_GAIN = 1.0
for e in sfx:
    e["volume"] = round(min(1.0, e["volume"] * SFX_GAIN), 3)
sfx = [e for e in sfx if e["volume"] > 0]

# --- Piste voix ------------------------------------------------------------------
# Son de la page des moments (panneau « Son », work/son.json) : volume de la voix de tout le
# montage, nettoyage (bruit de fond, clics, graves, clarté) et bruits parasites à atténuer
# (fenêtres en temps du rush) ; volume de la voix par moment : « gain » de chaque moment.
try:
    SOUND = json.load(open(os.path.join(WORK, "son.json")))
except (OSError, ValueError):
    SOUND = {}
VOICE_GAIN = max(0.0, min(2.0, float(SOUND.get("voice", 1) or 0)))
MUTE = [(float(a), float(b)) for a, b in SOUND.get("mute", []) if float(b) > float(a)]
os.makedirs(os.path.join(ROOT, "public", "audio"), exist_ok=True)
parts = []
for c in clips:
    s = int(c["trimBefore"] / FPS * SR)
    e = s + int(c["durationInFrames"] / FPS * SR)
    part = audio[s:e].copy() * VOICE_GAIN * PAGE_GAINS.get(c["seg"], 1.0)
    for ma, mb in MUTE:  # bruit parasite atténué de 15 dB, avec 10 ms de fondu de part et d'autre
        a, b = int(ma * SR) - s, int(mb * SR) - s
        if b <= 0 or a >= len(part):
            continue
        env = np.ones(len(part), np.float32)
        ramp = int(SR * 0.01)
        lo, hi = max(0, a - ramp), min(len(part), b + ramp)
        env[lo:hi] = 0.18
        for k in range(ramp):
            if 0 <= a - ramp + k < len(part):
                env[a - ramp + k] = 1 - (1 - 0.18) * k / ramp
            if 0 <= b + k < len(part):
                env[b + k] = 0.18 + (1 - 0.18) * k / ramp
        part *= env
    fade = int(SR * 0.004)  # micro-fondu anti-clic sur les coupes sèches
    part[:fade] *= np.linspace(0, 1, fade)
    part[-fade:] *= np.linspace(1, 0, fade)
    parts.append(part)
voice = np.concatenate(parts)
dry_path = os.path.join(WORK, "voice_dry.wav")
with wave.open(dry_path, "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes((np.clip(voice, -1, 1) * 32767).astype(np.int16).tobytes())

# Les graves parasites et la clarté sont faits au rendu par HyperFrames (selection/hf_audio.js,
# comme dans l'aperçu de la page) ; le bruit de fond et les clics, ici par ffmpeg.
clean = ""
DENOISE = {1: 6, 2: 12, 3: 20}.get(int(SOUND.get("denoise", 0) or 0))
if SOUND.get("declick"):
    clean += "adeclick=w=40:o=75,"
if DENOISE:
    clean += f"afftdn=nr={DENOISE}:nf=-50:tn=1,"
# 1. Nettoyage de toute la voix (panneau « Son de la voix »).
clean_path = os.path.join(WORK, "voice_clean.wav")
subprocess.run([FFMPEG, "-v", "error", "-y", "-i", dry_path, "-af", (clean or "anull,").rstrip(","),
                "-ar", str(SR), "-ac", "1", clean_path], check=True)


def read_wav(path):
    with wave.open(path) as w:
        return np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32) / 32768


def write_wav(path, x):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes((np.clip(x, -1, 1) * 32767).astype(np.int16).tobytes())


# 2. Voix modifiées faites par ffmpeg (hors HF_VOICES, faites au rendu) : chaque fenêtre est
#    traitée à part avec sa voix et sa force (moments_lib.voice_filter), puis remise en place
#    avec de courts fondus enchaînés.
track = read_wav(clean_path)
XF = int(SR * 0.02)
for f in funny:
    if f["voice"] in HF_VOICES:
        continue
    a, b = max(0, int(f["start"] * SR) - XF), min(len(track), int(f["end"] * SR) + XF)
    if b - a < 2 * XF + 10:
        continue
    seg = track[a:b].copy()
    seg_in, seg_out = os.path.join(WORK, "voice_fx_in.wav"), os.path.join(WORK, "voice_fx_out.wav")
    graph = voice_filter(f["voice"], f["force"])
    if f["voice"] == "batterie":  # ralentissement en Python, puis retour à la durée d'origine
        slow = batterie_warp(seg, f["force"])
        write_wav(seg_in, slow)
        graph = graph.replace("[0:a]", f"[0:a]rubberband=tempo={len(slow) / len(seg):.4f},", 1)
    else:
        write_wav(seg_in, seg)
    subprocess.run([FFMPEG, "-v", "error", "-y", "-i", seg_in, "-filter_complex", graph, "-map", "[r]",
                    "-ar", str(SR), "-ac", "1", seg_out], check=True)
    wet = read_wav(seg_out)[:len(seg)]
    wet = np.pad(wet, (0, len(seg) - len(wet)))
    ramp = np.ones(len(seg), np.float32)
    ramp[:XF] = np.linspace(0, 1, XF)
    ramp[-XF:] = np.linspace(1, 0, XF)
    track[a:b] = seg * (1 - ramp) + wet * ramp
    for p in (seg_in, seg_out):
        os.remove(p)
# Limiteur (-1,5 dB) : au rendu, par HyperFrames, après les voix modifiées.
write_wav(os.path.join(ROOT, "public", "audio", "voice.wav"), track)
os.remove(clean_path)

# --- Habillage de tout le montage ------------------------------------------------------
try:
    _h = json.load(open(os.path.join(WORK, "habillage.json")))
except (OSError, ValueError):
    _h = {}
HABILLAGE = {"autoZoom": max(0.0, min(2.0, float(_h.get("autoZoom", 1)))), "pop": bool(_h.get("pop", False)),
             "karaoke": bool(_h.get("karaoke", False)),
             "karaokeColor": _h.get("karaokeColor") if isinstance(_h.get("karaokeColor"), str) else "#ffd84d"}

# --- Sorties -----------------------------------------------------------------------
caption_style = fonts_lib.copy_for_render(fonts_lib.read_style())
WIDTH, HEIGHT = frame_size(ROOT)
edit = {
    "fps": FPS,
    # Format de la composition, celui du rush (portrait 1080x1920 ou paysage 1920x1080).
    "width": WIDTH,
    "height": HEIGHT,
    "durationInFrames": total_frames,
    "clips": clips,
    "hdRanges": hd_ranges,
    # Effets proposés par défaut sur la page des moments (segment -> effets).
    "sfxDefaults": {str(k): v for k, v in SFX_DEFAULTS.items()},
    "overlayDefaults": OVERLAY_DEFAULTS,
    "typed": typed,
    "captions": [{**c, "start": round(c["start"], 3), "end": round(c["end"], 3)} for c in captions],
    # Style des sous-titres choisi sur la page des moments (scripts/fonts_lib.py).
    "captionStyle": caption_style,
    "zooms": zooms,
    "zoomFx": sorted(zoom_fx, key=lambda z: z["start"]),
    "shakes": shakes,
    "flashes": flashes,
    "highlights": highlights,
    # Habillage de tout le montage (page des moments, work/habillage.json) : intensité des
    # zooms automatiques, sous-titres qui rebondissent, mot prononcé en couleur.
    "habillage": HABILLAGE,
    "funny": funny,
    # Nettoyage fait au rendu par HyperFrames (selection/hf_audio.js) : graves parasites, clarté.
    "son": {"highpass": bool(SOUND.get("highpass")), "clarity": bool(SOUND.get("clarity"))},
    "chips": chips,
    "sfx": sorted(sfx, key=lambda x: x["start"]),
}
os.makedirs(os.path.join(ROOT, "src", "data"), exist_ok=True)
json.dump(edit, open(os.path.join(ROOT, "src", "data", "edit.json"), "w"), ensure_ascii=False, indent=1)

print(f"{len(clips)} clips, {total_frames} images = {total_frames / FPS:.1f} s")
print(f"{len(captions)} sous-titres, {len(zooms)} zooms, {len(funny)} moments gênés, {len(sfx)} bruitages")
for c in captions:
    print(f"  {c['start']:6.2f} {c['top']} / {c['bottom']}")
