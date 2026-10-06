"""Visages du rush et cadrage des zooms (« tout le monde dans l'image »).

Sortie : src/data/face.json -> [{t, x, y, r, s, faces}] toutes les 0,5 s, en fractions de l'image
(version de travail, au format du rush) :
  x, y   point de cadrage : centre de la zone à garder (tous les visages retenus) ; les zooms
         sont centrés dessus (transform-origin) ;
  s      zoom maximal pour que cette zone reste dans l'image (1 = pas de zoom possible, 3 au plus) ;
         le rendu et l'aperçu de la page des moments plafonnent tous les zooms à s ;
  r      demi-largeur du plus grand visage ;
  faces  [[personne, x, y, w, h(, x0, y0, x1, y1)], …] : chaque visage (centre, largeur, hauteur) ;
         « personne » : numéro de la personne (1 = la plus présente), reconnue à son visage (SFace)
         d'un plan à l'autre, avec sa vignette dans work/visages/p<personne>.jpg (cadrage choisi par
         moment sur la page des moments) ; x0…y1 : cadre de la personne quand l'image en montre
         plusieurs (écran partagé, incrustation, cadres sur un fond), dont le plan rapproché ne
         sort pas.
Détecteur : YuNet (cv2.FaceDetectorYN, work/models/face_detection_yunet_2023mar.onnx, téléchargé
par prepare.sh, avec SFace pour reconnaître les personnes), qui trouve aussi les petits visages ; sans lui, cascade de
Haar d'OpenCV (un seul visage, le plus grand). Visages retenus pour le cadrage : ceux d'au moins
30 % de la taille du plus grand (pas le public ni les gens au fond). Les trous (visage caché par
une main, etc.) sont comblés par interpolation.

Reprise : les détections sont enregistrées au fil de l'eau (work/reprise/visage.json) ; une
préparation mise en pause ou coupée (AM_REPRISE=1) repart de la dernière ; elles restent là
une fois toutes faites, jusqu'à la fin de la préparation (effacées avec work/reprise).

Cadres (panel) : quand plusieurs visages sont dans l'image, le cadre de chacun est le plus grand
rectangle autour de lui dont les quatre côtés sont des bords droits présents à au moins 80 % de
leur longueur pendant ±4 s (ou les bords de l'image), qui ne contient pas d'autre visage, et
qu'aucune autre ligne droite ne traverse (ce serait un autre bord). Une image filmée (deux
personnes sur un plateau) n'en a pas : aucun rectangle de ce genre autour des visages.
"""
import bisect
import json
import os
import time

import cv2
import numpy as np

from progress import report

ROOT = os.path.join(os.path.dirname(__file__), "..")
SRC = os.path.join(ROOT, "public", "rushes", "rush_1080.mp4")
OUT = os.path.join(ROOT, "src", "data", "face.json")
MODELS = os.path.join(ROOT, "work", "models")
YUNET = os.path.join(MODELS, "face_detection_yunet_2023mar.onnx")
HAAR = os.path.join(MODELS, "haarcascade_frontalface_default.xml")
SFACE = os.path.join(MODELS, "face_recognition_sface_2021dec.onnx")
THUMBS = os.path.join(ROOT, "work", "visages")
CHECKPOINT = os.path.join(ROOT, "work", "reprise", "visage.json")
SAME_PERSON = 0.363  # similarité (cosinus) SFace au-dessus de laquelle deux visages sont la même personne
STEP = 0.5
DETECT_LONG = 640  # image réduite pour YuNet : grand côté de 640 px
MIN_FACE = 0.025  # visage plus petit que 2,5 % du petit côté de l'image : ignoré
MIN_SCORE = 0.7
KEEP_RATIO = 0.3  # visages retenus pour le cadrage : au moins 30 % de la largeur du plus grand
MAX_GAP = 2.0  # un visage perdu plus longtemps commence une nouvelle piste
MIN_SEEN = 2  # visage vu une seule fois : fausse détection probable, ignoré
LINK = 2.5
MAX_SCALE = 3.0
DEFAULT = (0.5, 0.4, 0.2)
PANEL_LONG = 320  # images réduites pour chercher les cadres : grand côté de 320 px
PANEL_EDGE = 20  # contraste d'un bord (dérivée de Sobel, canal le plus contrasté)
PANEL_LINE = 0.8  # côté d'un cadre : bord présent sur 80 % de sa longueur
PANEL_CROSS = 0.75  # ligne qui traverse un cadre (75 % de sa largeur ou hauteur) : pas un seul cadre
PANEL_R = 8  # instants voisins (±4 s) : un bord de cadre est fixe, pas ceux de l'image filmée  # aucun visage : zooms centrés sur le haut du milieu de l'image


def margin_box(x, y, w, h, alone):
    """Zone à garder autour d'un visage. Seul dans l'image : le visage lui-même (les zooms restent
    ceux d'avant, centrés sur lui, sauf s'ils le coupaient). À plusieurs : le visage avec un peu
    d'air (cheveux, menton), pour que personne ne sorte de l'image."""
    if alone:
        return x - 0.5 * w, y - 0.5 * h, x + 0.5 * w, y + 0.5 * h
    return x - 0.75 * w, y - 0.8 * h, x + 0.75 * w, y + 0.9 * h


def max_scale(px, py, box):
    """Zoom maximal centré sur (px, py) qui garde la zone box = (x0, y0, x1, y1) dans l'image :
    avec l'origine du zoom en p, l'image visible va de p·(1 - 1/s) à p + (1 - p)/s."""
    x0, y0, x1, y1 = max(0.0, box[0]), max(0.0, box[1]), min(1.0, box[2]), min(1.0, box[3])
    s = MAX_SCALE
    for p, a, b in ((px, x0, x1), (py, y0, y1)):
        if p - a > 1e-6:
            s = min(s, p / (p - a))
        if b - p > 1e-6:
            s = min(s, (1 - p) / (b - p))
    return max(1.0, s)


def detect_yunet(cap, w, h):
    """Visages d'une image : [(x, y, w, h, empreinte)] en fractions de l'image ; empreinte : vecteur
    SFace normalisé (qui est qui), None sans le modèle de reconnaissance."""
    k = DETECT_LONG / max(w, h)
    size = (max(1, int(w * k)), max(1, int(h * k)))
    yn = cv2.FaceDetectorYN.create(YUNET, "", size, MIN_SCORE)
    sf = cv2.FaceRecognizerSF.create(SFACE, "") if hasattr(cv2, "FaceRecognizerSF") and os.path.exists(SFACE) else None
    short = min(size)

    def run(frame):
        small = cv2.resize(frame, size)
        _, found = yn.detect(small)
        out = []
        for f in [] if found is None else found:
            fx, fy, fw, fh = (float(v) for v in f[:4])
            if min(fw, fh) >= MIN_FACE * short:
                emb = None
                if sf is not None:
                    emb = sf.feature(sf.alignCrop(small, f)).flatten()
                    emb = emb / (np.linalg.norm(emb) or 1)
                out.append(((fx + fw / 2) / size[0], (fy + fh / 2) / size[1], fw / size[0], fh / size[1], emb))
        return out
    return run


def detect_haar(cap, w, h):
    cascade = cv2.CascadeClassifier(HAAR)
    sw, sh = int(w) // 2 or 540, int(h) // 2 or 960

    def run(frame):
        gray = cv2.cvtColor(cv2.resize(frame, (sw, sh)), cv2.COLOR_BGR2GRAY)
        faces = cascade.detectMultiScale(gray, 1.1, 6, minSize=(90, 90))
        if not len(faces):
            return []
        x, y, fw, fh = max(faces, key=lambda f: f[2] * f[3])
        return [((x + fw / 2) / sw, (y + fh / 2) / sh, fw / sw, fh / sh, None)]
    return run


def edge_maps(frame, size):
    """Bords verticaux et horizontaux nets d'une image réduite (maximum local de la dérivée)."""
    g = cv2.resize(frame, size, interpolation=cv2.INTER_AREA).astype(np.float32)
    gx = np.abs(cv2.Sobel(g, cv2.CV_32F, 1, 0, ksize=3)).max(axis=2)
    gy = np.abs(cv2.Sobel(g, cv2.CV_32F, 0, 1, ksize=3)).max(axis=2)
    v = (gx > PANEL_EDGE) & (gx >= np.roll(gx, 1, 1)) & (gx >= np.roll(gx, -1, 1))
    h = (gy > PANEL_EDGE) & (gy >= np.roll(gy, 1, 0)) & (gy >= np.roll(gy, -1, 0))
    return v, h


def panels(maps, faces):
    """Cadre de chaque visage (x0, y0, x1, y1 en fractions de l'image) ou None. maps : bords des
    instants voisins (edge_maps) ; faces : [(x, y, w, h, …)]."""
    V = np.mean([m[0] for m in maps], axis=0) > 0.75  # bords fixes
    Hh = np.mean([m[1] for m in maps], axis=0) > 0.75
    V = V | np.roll(V, 1, 1) | np.roll(V, -1, 1)  # à un pixel près
    Hh = Hh | np.roll(Hh, 1, 0) | np.roll(Hh, -1, 0)
    H, W = V.shape
    cV = np.vstack([np.zeros((1, W)), np.cumsum(V, axis=0)])
    cH = np.hstack([np.zeros((H, 1)), np.cumsum(Hh, axis=1)])

    def vcov(c, a, b):  # part de la colonne c couverte par un bord entre les lignes a et b
        return 1.0 if c in (0, W) else (cV[b, c] - cV[a, c]) / max(1, b - a)

    def hcov(r, a, b):
        return 1.0 if r in (0, H) else (cH[r, b] - cH[r, a]) / max(1, b - a)

    def peaks(cands, cov):
        return [c for c in cands if cov(c) >= PANEL_LINE and cov(c) >= max(cov(c - 1), cov(c + 1))]

    out = []
    mx, my = int(0.03 * W), int(0.03 * H)
    for j, f in enumerate(faces):
        others = [(o[0] * W, o[1] * H) for i, o in enumerate(faces) if i != j]
        l0, t0 = max(0, int((f[0] - f[2] / 2) * W)), max(0, int((f[1] - f[3] / 2) * H))
        r0, b0 = min(W, int(np.ceil((f[0] + f[2] / 2) * W))), min(H, int(np.ceil((f[1] + f[3] / 2) * H)))
        # Côtés possibles : bords qui couvrent la hauteur (largeur) du visage, et bords de l'image.
        lefts = [0] + peaks(range(2, l0 - 1), lambda c: vcov(c, t0, b0))
        rights = [W] + peaks(range(r0 + 2, W - 2), lambda c: vcov(c, t0, b0))
        tops = [0] + peaks(range(2, t0 - 1), lambda r: hcov(r, l0, r0))
        bots = [H] + peaks(range(b0 + 2, H - 2), lambda r: hcov(r, l0, r0))
        best = None
        for l in lefts:
            for r in rights:
                if r - l < 0.15 * W:
                    continue
                for t in tops:
                    if hcov(t, l, r) < PANEL_LINE:
                        continue
                    for b in bots:
                        area = (r - l) * (b - t) / (W * H)
                        if b - t < 0.15 * H or area > 0.8:
                            continue
                        real = (l > 0) + (t > 0) + (r < W) + (b < H)  # côtés qui ne sont pas l'image
                        if best and (real, area) <= best[0]:
                            continue
                        if hcov(b, l, r) < PANEL_LINE or vcov(l, t, b) < PANEL_LINE or vcov(r, t, b) < PANEL_LINE:
                            continue
                        if any(l <= ox <= r and t <= oy <= b for ox, oy in others):
                            continue
                        # (hors des bandes de 3 % le long des côtés : bord double, séparateur épais)
                        if any(hcov(y, l + mx, r - mx) >= PANEL_CROSS for y in range(t + my, b - my + 1)) or \
                                any(vcov(x, t + my, b - my) >= PANEL_CROSS for x in range(l + mx, r - mx + 1)):
                            continue
                        best = ((real, area), l, t, r, b)
        if best is None:
            out.append(None)
            continue
        _, l, t, r, b = best
        # Resserré sur un bord tout proche à l'intérieur (cadre posé sur un fond, séparateur épais) :
        # mieux vaut perdre un liseré de l'image que montrer le bord.
        if l + mx < l0:
            l = max([x for x in range(l + 1, l + mx) if vcov(x, t, b) >= 0.5], default=l)
        if r - mx > r0:
            r = min([x for x in range(r - mx + 1, r) if vcov(x, t, b) >= 0.5], default=r)
        if t + my < t0:
            t = max([y for y in range(t + 1, t + my) if hcov(y, l, r) >= 0.5], default=t)
        if b - my > b0:
            b = min([y for y in range(b - my + 1, b) if hcov(y, l, r) >= 0.5], default=b)
        # un pixel de marge (bord flou de l'image réduite)
        l, t, r, b = l + (l > 0), t + (t > 0), r - (r < W), b - (b < H)
        out.append((l / W, t / H, r / W, b / H))
    return out


def track_panels(track, span):
    """Cadre d'une piste à chaque instant de span : celui trouvé à la plupart des instants voisins
    (±3 s), pris au plus serré des valeurs habituelles (un bord manqué une fois ne l'agrandit pas) ;
    {t: (x0, y0, x1, y1)}."""
    def usual(found):
        a = np.array(found)
        return (float(np.percentile(a[:, 0], 70)), float(np.percentile(a[:, 1], 70)),
                float(np.percentile(a[:, 2], 30)), float(np.percentile(a[:, 3], 30)))

    seen = sorted(track)
    every = [track[u][5] for u in seen if track[u][5]]
    # Cadre habituel de la piste (une piste ne passe pas d'un plan à l'autre) : pour les instants
    # où le bord a été masqué (bras, ombre), si le visage reste dedans.
    whole = usual(every) if len(every) >= 0.25 * len(seen) else None
    out = {}
    for t in span:
        near = [track[u][5] for u in seen[bisect.bisect_left(seen, t - 3):bisect.bisect_right(seen, t + 3)]]
        found = [p for p in near if p]
        if found and len(found) >= 0.4 * len(near):
            out[t] = usual(found)
        elif whole:
            f = track[min(seen, key=lambda u: abs(u - t))]
            if whole[0] <= f[0] <= whole[2] and whole[1] <= f[1] <= whole[3]:
                out[t] = whole
    return out


def link(samples):
    """Relie les visages d'un instant à l'autre (le plus proche, à moins de LINK largeurs de visage :
    la caméra peut bouger vite) :
    pistes = {id: {t: (x, y, w, h)}}."""
    tracks, last = {}, {}
    for t, found in samples:
        free = [i for i, (lt, _) in last.items() if t - lt <= MAX_GAP]
        pairs = sorted(((np.hypot(f[0] - last[i][1][0], f[1] - last[i][1][1]), n, i)
                        for n, f in enumerate(found) for i in free), key=lambda p: p[0])
        used_f, used_t = set(), set()
        for d, n, i in pairs:
            if n in used_f or i in used_t or d > LINK * max(found[n][2], last[i][1][2]):
                continue
            ea, eb = found[n][4], last[i][1][4]
            if ea is not None and eb is not None and float(ea @ eb) < SAME_PERSON / 2:
                continue  # proches mais pas la même personne (changement de plan)
            used_f.add(n)
            used_t.add(i)
            tracks[i][t] = found[n]
            last[i] = (t, found[n])
        for n, f in enumerate(found):
            if n not in used_f:
                i = len(tracks)
                tracks[i] = {t: f}
                last[i] = (t, f)
    return {i: tr for i, tr in tracks.items() if len(tr) >= MIN_SEEN or len(tracks) == 1}


def persons(tracks):
    """Regroupe les pistes par personne (empreintes SFace moyennes proches) : {piste: personne},
    personnes numérotées à partir de 1, de la plus présente à la moins présente. Sans empreintes,
    une piste = une personne."""
    order = sorted(tracks, key=lambda i: -len(tracks[i]))
    groups = []  # [(somme des empreintes, [pistes])]
    for i in order:
        embs = [f[4] for f in tracks[i].values() if f[4] is not None]
        mean = np.mean(embs, axis=0) if embs else None
        if mean is not None:
            mean = mean / (np.linalg.norm(mean) or 1)
            best = max(((float(mean @ (g[0] / np.linalg.norm(g[0]))), k) for k, g in enumerate(groups) if g[0] is not None),
                       default=(-1, None))
            if best[0] >= SAME_PERSON:
                g = groups[best[1]]
                groups[best[1]] = (g[0] + mean * len(embs), g[1] + [i])
                continue
        groups.append((None if mean is None else mean * len(embs), [i]))
    groups.sort(key=lambda g: -sum(len(tracks[i]) for i in g[1]))
    return {i: k + 1 for k, g in enumerate(groups) for i in g[1]}


def thumbnails(cap, fps, tracks, who):
    """Vignette de chaque personne (work/visages/p<n>.jpg, 160 px) : son plus grand visage."""
    import shutil
    shutil.rmtree(THUMBS, ignore_errors=True)
    os.makedirs(THUMBS)
    best = {}
    for i, tr in tracks.items():
        for t, f in tr.items():
            if who[i] not in best or f[2] > best[who[i]][1][2]:
                best[who[i]] = (t, f)
    for p, (t, f) in best.items():
        cap.set(cv2.CAP_PROP_POS_FRAMES, int(t * fps))
        ok, frame = cap.read()
        if not ok:
            continue
        H, W = frame.shape[:2]
        half = max(f[2] * W, f[3] * H) * 0.8
        x0, y0 = int(max(0, f[0] * W - half)), int(max(0, f[1] * H - half))
        crop = frame[y0:int(min(H, f[1] * H + half)), x0:int(min(W, f[0] * W + half))]
        if crop.size:
            cv2.imwrite(os.path.join(THUMBS, f"p{p}.jpg"), cv2.resize(crop, (160, 160)), [cv2.IMWRITE_JPEG_QUALITY, 85])


def smooth(values, n=5):
    v = np.array(values, float)
    if len(v) < n:
        return v
    return np.convolve(np.pad(v, n // 2, mode="edge"), np.ones(n) / n, mode="valid")


def track_positions(track, times):
    """Positions d'une piste à chaque instant entre sa première et sa dernière détection (trous
    de moins de MAX_GAP comblés), lissées : {t: (x, y, w, h)}."""
    seen = sorted(track)
    span = [t for t in times if seen[0] <= t <= seen[-1]]
    cols = [np.interp(span, seen, [track[t][k] for t in seen]) for k in range(4)]
    cols = [smooth(c) for c in cols]
    out = {}
    for j, t in enumerate(span):
        k = bisect.bisect_left(seen, t)
        before = seen[k - 1] if k and seen[k] != t else t
        after = seen[k]
        if after - before <= MAX_GAP:
            out[t] = tuple(float(c[j]) for c in cols)
    return out


def main():
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    cap = cv2.VideoCapture(SRC)
    fps = cap.get(cv2.CAP_PROP_FPS)
    n = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
    w, h = cap.get(cv2.CAP_PROP_FRAME_WIDTH), cap.get(cv2.CAP_PROP_FRAME_HEIGHT)
    if hasattr(cv2, "FaceDetectorYN") and os.path.exists(YUNET):
        detect, name = detect_yunet(cap, w, h), "YuNet"
    elif hasattr(cv2, "CascadeClassifier") and os.path.exists(HAAR):
        detect, name = detect_haar(cap, w, h), "Haar (un visage)"
    else:
        detect, name = None, None
        print(f"Détection du visage indisponible (OpenCV {cv2.__version__}, modèles absents) : zooms centrés sur l'image.")

    times = [round(float(t), 2) for t in np.arange(0, n / fps, STEP)] if fps and n else []
    samples, maps = [], []
    key = f"{name} {n} {fps} {w}x{h} {os.path.getsize(SRC) if os.path.exists(SRC) else 0}"
    try:
        saved = json.load(open(CHECKPOINT))
        if os.environ.get("AM_REPRISE") == "1" and saved["key"] == key:
            samples = [(t, [(*f[:4], None if f[4] is None else np.array(f[4], np.float32), f[5]) for f in found])
                       for t, found in saved["samples"]]
            print(f"Reprise de la position du visage à {samples[-1][0] if samples else 0:.0f} s", flush=True)
    except (OSError, ValueError, KeyError, TypeError, IndexError):
        pass

    def checkpoint(done):
        """Enregistre les done premiers instants (cadres déjà cherchés)."""
        os.makedirs(os.path.dirname(CHECKPOINT), exist_ok=True)
        json.dump({"key": key, "samples": [
            (t, [(*map(float, f[:4]), None if f[4] is None else [round(float(v), 5) for v in f[4]],
                  None if f[5] is None else list(f[5])) for f in found]) for t, found in samples[:done]]},
            open(CHECKPOINT + ".part", "w"))
        os.replace(CHECKPOINT + ".part", CHECKPOINT)
    k = PANEL_LONG / max(w, h) if w and h else 1
    small = (max(1, int(w * k)), max(1, int(h * k)))

    def add_panels(j):
        """Cadres des visages de l'instant j (quand il y en a plusieurs), d'après les bords des
        instants voisins (maps garde les PANEL_R précédents et suivants)."""
        t, found = samples[j]
        off = len(samples) - len(maps)  # instant de maps[0]
        near = maps[max(0, j - PANEL_R - off):j + PANEL_R + 1 - off]
        found_p = panels(near, found) if len(found) > 1 else [None] * len(found)
        samples[j] = (t, [(*f[:5], p) for f, p in zip(found, found_p)])

    resumed, saved_at = len(samples), time.monotonic()
    for t in times[resumed:]:
        if detect is None:
            break
        report("Position du visage", t, n / fps)
        if time.monotonic() - saved_at > 20:
            checkpoint(max(resumed, len(samples) - PANEL_R))
            saved_at = time.monotonic()
        cap.set(cv2.CAP_PROP_POS_FRAMES, int(t * fps))
        ok, frame = cap.read()
        if not ok:
            break
        samples.append((t, detect(frame)))
        maps = (maps + [edge_maps(frame, small)])[-(2 * PANEL_R + 1):]
        if len(samples) - 1 - PANEL_R >= resumed:
            add_panels(len(samples) - 1 - PANEL_R)
    for j in range(max(resumed, len(samples) - PANEL_R), len(samples)):
        add_panels(j)
    if detect is not None:
        report("Position du visage", n / fps, n / fps, every=0)
        # Détections gardées jusqu'à la fin de la préparation : une préparation abandonnée en
        # gardant la position du visage (page « Rushes et montages ») la reprend sans rien refaire.
        checkpoint(len(samples))
    if not times:  # durée illisible : celle du son (écrit par prepare.sh juste avant)
        import wave
        with wave.open(os.path.join(ROOT, "work", "rush_16k.wav")) as wav:
            times = [round(float(t), 2) for t in np.arange(0, wav.getnframes() / wav.getframerate() + STEP, STEP)]

    linked = link(samples)
    who = persons(linked)
    if detect is not None:
        thumbnails(cap, fps, linked, who)
    tracks = {i: track_positions(tr, times) for i, tr in linked.items()}
    frames_panels = {i: track_panels(tr, list(tracks[i])) for i, tr in linked.items()}
    frames = []  # (t, visages présents [(personne, x, y, w, h)], cadres {personne: (x0, y0, x1, y1)})
    for t in times:
        frames.append((t, [(who[i], *pos[t]) for i, pos in tracks.items() if t in pos],
                       {who[i]: p[t] for i, p in frames_panels.items() if t in p}))
    # Cadre manqué (bord masqué un moment, piste coupée) : celui de la même personne trouvé le plus
    # près dans le temps (15 s au plus), si son visage est dedans.
    known_p = {}
    for t, _, boxes in frames:
        for who_, b in boxes.items():
            known_p.setdefault(who_, []).append((t, b))
    for t, faces, boxes in frames:
        for f in faces if len(faces) > 1 else ():
            if f[0] in boxes or f[0] not in known_p:
                continue
            u, b = min(known_p[f[0]], key=lambda kb: abs(kb[0] - t))
            if abs(u - t) <= 15 and b[0] <= f[1] <= b[2] and b[1] <= f[2] <= b[3]:
                boxes[f[0]] = b

    # Zone à garder à chaque instant : visages retenus (au moins KEEP_RATIO du plus grand).
    raw = []
    for t, faces, _ in frames:
        if not faces:
            raw.append(None)
            continue
        big = max(f[3] for f in faces)
        kept = [f for f in faces if f[3] >= KEEP_RATIO * big]
        boxes = [margin_box(*f[1:], len(kept) == 1) for f in kept]
        box = (min(b[0] for b in boxes), min(b[1] for b in boxes), max(b[2] for b in boxes), max(b[3] for b in boxes))
        raw.append((box, big / 2))
    known = [k for k, v in enumerate(raw) if v]
    if not known:
        print("Aucun visage détecté : zooms centrés sur l'image.")
    res = []
    for k, (t, faces, boxes) in enumerate(frames):
        if raw[k] is None and known:  # pas de visage : zone du plus proche instant avec visage
            raw[k] = raw[min(known, key=lambda j: abs(j - k))]
        if raw[k] is None:
            x, y, r = DEFAULT
            res.append({"t": t, "x": x, "y": y, "r": r, "s": MAX_SCALE, "faces": []})
            continue
        box, r = raw[k]
        x, y = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
        res.append({"t": t, "x": x, "y": y, "r": r, "box": box,
                    "faces": [[i, round(fx, 4), round(fy, 4), round(fw, 4), round(fh, 4)]
                              + [round(v, 4) for v in boxes.get(i, ())] for i, fx, fy, fw, fh in faces]})
    if res:
        # Lissage du point de cadrage ; zoom maximal pris au plus serré des instants voisins
        # (le zoom se réduit avant qu'une deuxième personne entre dans l'image).
        for key in ("x", "y", "r"):
            for p, v in zip(res, smooth([p[key] for p in res])):
                p[key] = round(float(v), 4)
        caps = [max_scale(p["x"], p["y"], p["box"]) if "box" in p else p["s"] for p in res]
        for j, p in enumerate(res):
            p["s"] = round(min(caps[max(0, j - 2):j + 3]), 3)
            p.pop("box", None)
    json.dump(res, open(OUT, "w"))
    many = sum(1 for _, f, _ in frames if len(f) > 1)
    framed = sum(1 for _, _, b in frames if b)
    if known:
        print(f"{name} : visage trouvé à {len(known)}/{len(frames)} instants, plusieurs visages à {many}, "
              f"{len(set(who.values()))} personne(s) ({len(tracks)} pistes), chacune dans son cadre à {framed}, "
              f"zoom maximal médian ×{np.median([p['s'] for p in res]):.2f}")


if __name__ == "__main__":
    main()
