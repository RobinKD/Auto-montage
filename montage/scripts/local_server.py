"""Interface web locale (Docker ou poste) : page des moments, version de travail et téléchargements.

Usage : python3 scripts/local_server.py [--host 0.0.0.0] [--port 8080]
  /              page des moments (work/moments/index.html, générée par make_moments.py)
  /rush/         nouveau rush : choix du fichier (envoyé dans public/rushes/) et préparation
                 (prepare.sh, rendu de la version de travail, make_moments.py) par boutons
  /downloads/    versions 720p, 1080p et 4K à télécharger (out/)
  /chat/         discussion avec Claude Code sur le montage (claude -p, outils limités au
                 projet, conversation reprise d'un message à l'autre, historique dans work/chat/)
Les pages sont les mêmes que sur claude.ai ; local/shim.js leur fournit en local ce que
claude.ai leur donne : enregistrement des sélections, corrections et effets dans
work/local_db/<collection>/<id>.json (même forme que l'export ArtifactData, lu tel quel par
use_selection.py), et téléchargement direct. « Générer la version de travail » lance ici
use_selection.py, build_edit.py, render.sh apercu et make_moments.py, sans passer par Claude.
"""
import argparse
import datetime
import email.utils
import html as htmllib
import json
import mimetypes
import os
import re
import shutil
import subprocess
import signal
import sys
import threading
import time
import traceback
import unicodedata
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, unquote, urlparse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from join_rushes import joined_name  # noqa: E402
from progress import exit_reason, resources  # noqa: E402
import decoupage  # noqa: E402
import memoire  # noqa: E402
import place  # noqa: E402
import fonts_lib  # noqa: E402
from moments_lib import SOUNDS as DEFAULT_SOUNDS, DEFAULTS_VERSION, LANGUAGES, VISUALS, VOICES, edit_size, hf_preview_files, hidden_effects, language, res_label, sound_catalog, wav_len  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
WORK = os.path.join(ROOT, "work")
MOMENTS = os.path.join(WORK, "moments")
DB = os.path.join(WORK, "local_db")
OUT = os.path.join(ROOT, "out")
RUSHES = os.path.join(ROOT, "public", "rushes")
VIDEO_EXT = (".mov", ".mp4", ".m4v", ".mkv", ".webm", ".avi", ".mts")
DERIVED = re.compile(r"^rush_(1080|2160)\.")  # versions de travail, pas des rushes
SHIM = '<script src="/local/shim.js"></script>\n<script src="/local/menu.js"></script>\n'  # menu : bandeau des pages
# Version installée (fichier VERSION à la racine du dépôt), lue au démarrage : les lanceurs la
# comparent à celle des fichiers pour redémarrer le serveur après une mise à jour.
_version_file = os.path.join(ROOT, "..", "VERSION")
VERSION = open(_version_file).read().strip() if os.path.exists(_version_file) else "dev"
STARTED = time.time()  # démarrage du serveur : la page voit qu'il a redémarré (même version)


def disk_version():
    """Version des fichiers maintenant (différente de VERSION si le projet a changé sur le disque
    depuis le démarrage, ex. git pull dans le dossier d'installation : redémarrage à faire)."""
    try:
        return open(_version_file).read().strip()
    except OSError:
        return VERSION
# Même enveloppe que celle que claude.ai ajoute aux pages publiées.
SKELETON = ('<!doctype html><html lang="fr"><head><meta charset="utf-8">'
            '<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">'
            '<style>body{margin:0}img{max-width:100%}[hidden]{display:none!important}</style>'
            + f'<script>window.AUTO_MONTAGE_VERSION = {json.dumps(VERSION)};</script>'
            + SHIM + '</head><body>')
SAFE_ID = re.compile(r"^(?!\.)[A-Za-z0-9_.~:@+-]{1,200}$")  # ni « . » ni « .. »
mimetypes.add_type("video/mp4", ".mp4")
mimetypes.add_type("audio/mpeg", ".mp3")
mimetypes.add_type("font/ttf", ".ttf")


def inside(base, rel):
    """Chemin de rel sous base, ou None s'il en sort."""
    path = os.path.realpath(os.path.join(base, rel))
    return path if path == os.path.realpath(base) or path.startswith(os.path.realpath(base) + os.sep) else None


def read_text(path):
    return open(path, encoding="utf-8").read().strip() if os.path.exists(path) else None


# --- Style des sous-titres (page des moments) et effets proposés (page des rushes) -----
# Polices : scripts/fonts_lib.py. Les fichiers sont servis à la page par leur identifiant
# (/api/fonts/<id>), seulement s'ils font partie de la liste des polices utilisables.
EFFECTS_FILE = os.path.join(WORK, "effets_page.json")
# Effets fournis par défaut : mêmes identifiants que la page des moments (sfxCatalog, VFX_CATALOG).
PUBLIC_EFFECTS = [{"id": i, "label": label, "kind": "son"} for i, label, _, _ in DEFAULT_SOUNDS] + \
    [{"id": "voix", "label": "Voix modifiée", "kind": "voix"},
     {"id": "gainvoix", "label": "Volume de la voix par moment", "kind": "voix"}] + \
    [{"id": i, "label": label, "kind": "visuel"} for i, label in VISUALS]


def font_files():
    fonts = fonts_lib.list_fonts()
    return fonts, {fonts_lib.file_id(path): path for f in fonts for path in f["faces"].values()}


def caption_style_info():
    fonts, _ = font_files()
    style = fonts_lib.read_style()
    family = fonts_lib.resolve(style, fonts)
    return {"style": style | {"font": family["id"] if family else None},
            "fonts": [{"id": f["id"], "family": f["family"], "source": f["source"],
                       "faces": {str(w): f"/api/fonts/{fonts_lib.file_id(p)}" for w, p in sorted(f["faces"].items())}}
                      for f in fonts],
            "sizeRange": fonts_lib.SIZE_RANGE, "shadowRange": fonts_lib.SHADOW_RANGE, "blurRange": fonts_lib.BLUR_RANGE,
            "offsetMax": fonts_lib.OFFSET_MAX, "linesRange": fonts_lib.LINES_RANGE,
            "lineSizeRange": fonts_lib.LINE_SIZE_RANGE, "default": fonts_lib.DEFAULT}


def caption_style_for_page():
    """Style des sous-titres pour l'aperçu de la page (police servie par /api/fonts/)."""
    style = fonts_lib.read_style()
    family = fonts_lib.resolve(style)
    if not family:
        return None
    faces = fonts_lib.faces_for(family, style["weight"])
    return {"family": "AM Sous-titres", "label": family["family"],
            "files": [{"url": f"/api/fonts/{fonts_lib.file_id(p)}", "weight": str(w)} for w, p in faces],
            "weight": style["weight"], "size": style["size"], "uppercase": style["uppercase"], "color": style["color"],
            "shadow": style["shadow"], "blur": style["blur"], "ox": style["ox"], "oy": style["oy"],
            "lines": style["lines"], "lineSizes": style["lineSizes"]}


def style_videos():
    return sorted(n for n in os.listdir(STYLE_VIDEOS) if n.lower().endswith(VIDEO_EXT) and not n.startswith(".")) \
        if os.path.isdir(STYLE_VIDEOS) else []


def style_seconds():
    return sum(media_seconds(os.path.join(STYLE_VIDEOS, n)) for n in style_videos())


def style_analysis():
    """Analyse des vidéos d'exemple : mesures, résultat de Claude, durée prévue d'une analyse."""
    def load(name):
        try:
            return json.load(open(os.path.join(STYLE_ANALYSE, name)))
        except (OSError, ValueError):
            return None
    facts, result = load("faits.json"), load("resultat.json")
    videos = style_videos()
    rates = load_rates()
    seconds = style_seconds()
    estimate_s = rates.get("Analyse des vidéos d'exemple", 0.6) * seconds + rates.get("étape:mesures des vidéos d'exemple", 3) \
        + rates.get("étape:analyse par Claude", 150)
    analysed = [v["video"] for v in (facts or {}).get("videos", [])]
    return {"videos": videos, "seconds": round(seconds, 1), "estimate": round(estimate_s),
            "facts": facts, "result": result, "stale": bool(facts) and analysed != videos,
            "at": datetime.datetime.fromtimestamp(os.path.getmtime(os.path.join(STYLE_ANALYSE, "resultat.json"))).isoformat(timespec="seconds")
            if result else None}


def effects_file(key="hidden"):
    """Effets écartés (« hidden ») ou à décocher de tous les moments au prochain passage sur
    la page des moments (« purge », demandé depuis la page « Rushes et montages »)."""
    try:
        items = json.load(open(EFFECTS_FILE)).get(key, [])
        return [h for h in items if isinstance(h, str)]
    except (OSError, ValueError, AttributeError):
        return []


# Son de la voix (page des moments) : réglages du montage en cours dans work/son.json
# (volume, nettoyage, bruits à atténuer), analyse par scripts/analyze_sound.py.
SOUND_FILE = os.path.join(WORK, "son.json")


def sound_settings():
    try:
        data = json.load(open(SOUND_FILE))
        return data if isinstance(data, dict) else {}
    except (OSError, ValueError):
        return {}


HABILLAGE_FILE = os.path.join(WORK, "habillage.json")
TUTORIAL_SEEN = os.path.join(WORK, "tutoriel_vu")  # le tutoriel a été ouvert une fois  # zooms automatiques, animation des sous-titres


def habillage():
    try:
        h = json.load(open(HABILLAGE_FILE))
    except (OSError, ValueError):
        h = {}
    return {"autoZoom": 1, "pop": False, "karaoke": False, "karaokeColor": "#ffd84d"} | (h if isinstance(h, dict) else {})


def save_habillage(body):
    h = habillage()
    try:
        if "autoZoom" in body:
            h["autoZoom"] = round(max(0.0, min(2.0, float(body["autoZoom"]))), 2)
        for key in ("pop", "karaoke"):
            if key in body:
                h[key] = bool(body[key])
        if "karaokeColor" in body and re.fullmatch(r"#[0-9a-fA-F]{6}", str(body["karaokeColor"])):
            h["karaokeColor"] = str(body["karaokeColor"]).lower()
    except (TypeError, ValueError):
        return None
    os.makedirs(WORK, exist_ok=True)
    json.dump(h, open(HABILLAGE_FILE, "w"), indent=1)
    return h


def save_sound(body):
    s = sound_settings()
    try:
        if "voice" in body:
            s["voice"] = round(max(0.0, min(2.0, float(body["voice"]))), 2)
        if "denoise" in body:
            s["denoise"] = max(0, min(3, int(body["denoise"])))
        for key in ("highpass", "declick", "clarity"):
            if key in body:
                s[key] = bool(body[key])
        if "mute" in body:
            s["mute"] = [[round(float(a), 2), round(float(b), 2)] for a, b in body["mute"] if float(b) > float(a)][:200]
    except (TypeError, ValueError):
        return None
    os.makedirs(WORK, exist_ok=True)
    json.dump(s, open(SOUND_FILE, "w"), indent=1)
    return s


def effects_volume():
    """Volume de tous les effets sonores (1 = 100 %)."""
    try:
        return float(json.load(open(EFFECTS_FILE)).get("volume", 1))
    except (OSError, ValueError, TypeError, AttributeError):
        return 1.0


def effects_hidden():
    return hidden_effects(EFFECTS_FILE)  # sons par défaut écartés tant qu'ils ne sont pas cochés


def effects_info():
    hidden = set(effects_hidden())
    items = PUBLIC_EFFECTS + [{"id": s["id"], "label": s["label"].capitalize(), "kind": "perso"} for s in style_info()["sons"]]
    # Écoute sur la page « Rushes et montages » : fichier(s) de chaque son.
    files = {i: [f"/sfx-defaut/{f}"] for i, _, f, _ in DEFAULT_SOUNDS if f}
    files["clavier"] = [f"/sfx-defaut/key-{k}.wav" for k in (1, 2, 3, 4, 1, 2)]
    for e in items:
        if e["kind"] == "perso":
            files[e["id"]] = [f"/sfx/perso/{e['id'][6:]}.wav"]
    items = [e | ({"audio": files[e["id"]]} if e["id"] in files else {}) for e in items]
    return {"effects": [e | {"shown": e["id"] not in hidden} for e in items], "purge": effects_file("purge"),
            "volume": effects_volume()}


def moments_page():
    """Page des moments avec le gabarit actuel (selection/page.html) et les données du rush
    gardées dans work/moments/index.html : une mise à jour d'Auto-montage s'y voit tout de
    suite, sans refaire la préparation."""
    html = open(os.path.join(MOMENTS, "index.html"), encoding="utf-8").read()
    hf_preview_files(MOMENTS)  # fichiers hf/ de l'aperçu des voix (mis à jour avec Auto-montage)
    m = re.search(r"^  const DATA = (\{.*\});$", html, re.M)
    if not m:
        return html
    data = json.loads(m.group(1))
    # Sons personnels : ceux de la bibliothèque actuelle (ajoutés ou retirés depuis la dernière
    # génération), avec leur MP3 d'écoute dans la page.
    sons = {s["id"]: s for s in style_info()["sons"]}
    catalog = [c for c in data.get("sfxCatalog") or [] if not str(c.get("id", "")).startswith("perso-") or c["id"] in sons]
    # Sons par défaut ajoutés depuis la préparation de ce montage (nouvelle version) : dans le
    # catalogue, avec leur MP3 d'écoute, avant les sons personnels.
    default_ids = {c["id"] for c in sound_catalog()}
    catalog = sound_catalog() + [c for c in catalog if c.get("id") not in default_ids]
    data["voices"] = [{"id": i, "label": label, "desc": desc} for i, label, desc in VOICES]  # voix modifiées
    for c in catalog:  # durée des sons personnels (calage des effets sur la page)
        if str(c.get("id", "")).startswith("perso-") and not c.get("len"):
            c["len"] = wav_len(os.path.join(SOUNDS, c["id"][6:] + ".wav"))
    for i, _, f, _ in DEFAULT_SOUNDS:
        mp3 = os.path.join(MOMENTS, "sfx", f"{i}.mp3")
        if f and not os.path.exists(mp3) and os.path.exists(os.path.join(ROOT, "public", "sfx", f)):
            import imageio_ffmpeg
            os.makedirs(os.path.dirname(mp3), exist_ok=True)
            subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-v", "error", "-y", "-i", os.path.join(ROOT, "public", "sfx", f),
                            "-c:a", "libmp3lame", "-b:a", "96k", mp3], check=False)
    for sid, s in sons.items():
        mp3 = os.path.join(MOMENTS, "sfx", f"{sid}.mp3")
        if not os.path.exists(mp3):
            import imageio_ffmpeg
            os.makedirs(os.path.dirname(mp3), exist_ok=True)
            subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-v", "error", "-y", "-i", os.path.join(SOUNDS, sid[6:] + ".wav"),
                            "-c:a", "libmp3lame", "-b:a", "96k", mp3], check=False)
        if not any(c.get("id") == sid for c in catalog):
            catalog.append({"id": sid, "label": s["label"].capitalize(), "vol": 0.6})
    data["sfxCatalog"] = catalog
    data["hiddenEffects"] = effects_hidden()
    data["sfxVolume"] = effects_volume()
    data["habillage"] = habillage()
    data["captionStyle"] = caption_style_for_page()
    # Adresses des vidéos et vignettes suivies de leur date de modification : le navigateur
    # ne peut pas resservir la version de travail ou un extrait d'un rush ou d'un rendu
    # précédent (mêmes noms de fichiers d'une fois à l'autre).
    def fresh(rel):
        path = inside(MOMENTS, rel or "")
        return f"{rel}?v={os.stat(path).st_mtime_ns // 1000000}" if rel and path and os.path.isfile(path) else rel
    if isinstance(data.get("working"), dict):
        data["working"]["src"] = fresh(data["working"].get("src"))
    for mo in data.get("moments") or []:
        mo["clip"], mo["thumb"] = fresh(mo.get("clip")), fresh(mo.get("thumb"))
    rush = os.path.splitext(data.get("rush") or "rush")[0]
    data = json.dumps(data, ensure_ascii=False).replace("<", "\\u003c")  # pas de « </script> » dans les données
    tpl = open(os.path.join(ROOT, "selection", "page.html"), encoding="utf-8").read()
    return tpl.replace("__RUSH__", htmllib.escape(rush)).replace("/*__DATA__*/null", data)


def local_links(html):
    """Les liens vers les pages claude.ai pointent vers leurs équivalents locaux."""
    for name, local in (("moments_url.txt", "/"), ("downloads_url.txt", "/downloads/")):
        url = read_text(os.path.join(WORK, name))
        if url:
            html = html.replace(json.dumps(url), json.dumps(local))
    return html


# --- Tâches longues : génération de la version de travail, préparation d'un rush ------
# Une seule à la fois ; la page suit l'étape et la fin du journal (/api/job).
job = {"state": "idle", "kind": None, "step": "", "error": None, "log": [], "progress": None, "spent": {}}
job_lock = threading.Lock()
# Analyse des vidéos d'exemple : mesures (coupes, images datées, attaques sonores), puis
# lecture par Claude (ce qui a été repéré et ce qu'Auto-montage peut en reproduire).
STYLE_STEPS = [
    ("mesures des vidéos d'exemple", ["python3", "scripts/analyze_style.py"]),
    ("analyse par Claude", ["bash", "scripts/style_claude.sh"]),
]
STYLE_ANALYSE = os.path.join(WORK, "style", "analyse")
REGEN_STEPS = [
    ("lecture de la sélection", ["python3", "scripts/use_selection.py", DB, "{arg}"]),
    ("montage", ["python3", "scripts/build_edit.py"]),
    ("rendu 1080p", ["bash", "scripts/render.sh", "apercu"]),
    ("mise à jour de la page", ["python3", "scripts/make_moments.py"]),
]
PREPARE_STEPS = [
    ("préparation du rush", ["bash", "scripts/prepare.sh", "{args}"]),  # une ou plusieurs vidéos
    ("rendu de la version de travail", ["bash", "scripts/render.sh", "apercu"]),
    ("page des moments", ["python3", "scripts/make_moments.py"]),
]
# Version 4K (bouton des pages) : source 4K des seules images gardées, puis rendu final.
FINAL_STEPS = [
    ("source 4K", ["python3", "scripts/make_hd.py"]),
    ("rendu 4K", ["bash", "scripts/render.sh", "final"]),
]
# Avec les consignes de montage appliquées par Claude, entre la préparation et le rendu.
PREPARE_WITH_INSTRUCTIONS = PREPARE_STEPS[:1] + [
    ("consignes de montage (Claude)", ["bash", "scripts/apply_instructions.sh"]),
    ("montage selon les consignes", ["python3", "scripts/build_edit.py"]),
] + PREPARE_STEPS[1:]
INSTRUCTIONS = os.path.join(WORK, "instructions.md")
# Temps restant : chaque opération (barre) et le reste de chaque étape (hors barres) durent
# « vitesse × échelle » secondes ; l'échelle est la durée du rush, celle du montage pour le
# rendu, ou 1 (durée fixe). Les vitesses de départ viennent de mesures (rush de 5 min sur un
# poste, rush de 26 s sur 2 cœurs) ; elles sont recalées après chaque tâche réussie sur les
# durées réelles de la machine (work/timings.json), et l'opération en cours suit sa propre
# vitesse dès qu'elle a avancé.
PLAN = {"préparation du rush": ["Version de travail (MP4)", "Extraction du son", "Transcription", "Position du visage"],
        "rendu de la version de travail": ["Rendu de la vidéo"], "rendu 1080p": ["Rendu de la vidéo"],
        "page des moments": ["Découpage des moments"], "mise à jour de la page": ["Découpage des moments"],
        "source 4K": ["Source 4K"], "rendu 4K": ["Rendu 4K"],
        "mesures des vidéos d'exemple": ["Analyse des vidéos d'exemple"]}
RATES = {  # clé : (échelle, secondes par seconde d'échelle)
    "Version de travail (MP4)": ("rush", 0.3), "Extraction du son": ("rush", 0.01),
    "Transcription": ("rush", 0.3), "Position du visage": ("rush", 0.1),
    # Plusieurs vidéos pour un rush : mises bout à bout (copie) ou réencodées (réglages différents).
    "Assemblage des vidéos": ("rush", 0.03), "Assemblage des vidéos (réencodage)": ("rush", 1.2),
    "Rendu de la vidéo": ("montage", 3.7), "Découpage des moments": ("rush", 0.5),
    "étape:préparation du rush": ("rush", 0.15), "étape:consignes de montage (Claude)": ("fixe", 120),
    "étape:rendu de la version de travail": ("fixe", 20), "étape:rendu 1080p": ("fixe", 20),
    # 4K, mesuré sur 4 cœurs (rush de 5 min, montage de 39 s) : la source 4K relit tout le rush
    # (décodage 4K, ~1,9 s par seconde de rush) ; rendu 4K HyperFrames (2 navigateurs) ~13 à 30 s
    # par seconde de montage (montage de 18 s en 4 à 9 min sur 4 cœurs).
    # Analyse du style : échelle = durée totale des vidéos d'exemple ; Claude, durée fixe.
    "Analyse des vidéos d'exemple": ("rush", 0.6), "étape:mesures des vidéos d'exemple": ("fixe", 3),
    "étape:analyse par Claude": ("fixe", 150),
    "Source 4K": ("rush", 1.9), "Rendu 4K": ("montage", 20), "étape:rendu 4K": ("fixe", 20),
}
TIMINGS = os.path.join(WORK, "timings.json")
# Avancement affiché en temps de vidéo traité : unité de chaque opération (« rush » : secondes du
# rush, « montage » : secondes du montage) ; les autres sont des nombres (segments…).
UNITS = {k: kind for k, (kind, _) in RATES.items() if kind != "fixe"} | {"Segments transcrits seuls": "segments"}
# Pause et reprise (préparation d'un rush) : la tâche en cours est décrite dans work/tache.json
# (étapes, étape atteinte, avancement), mis à jour à chaque étape. « Mettre en pause » arrête les
# programmes en gardant ce qui est fait (morceaux de la version de travail, morceaux transcrits,
# positions du visage, extraits : work/reprise/, work/seg/, work/moments/clips/) ; « Reprendre »
# relance l'étape atteinte avec AM_REPRISE=1, qui repart de là. Une tâche coupée par un arrêt du
# serveur, du conteneur ou de l'ordinateur (fichier resté « running »), ou arrêtée par une erreur,
# se reprend de la même façon.
TASK = os.path.join(WORK, "tache.json")
RESUMABLE = {"prepare"}
INSTRUCTIONS_NAME = os.path.join(WORK, "instructions.name")


_claude_status = {"at": 0.0, "value": False}


def claude_connected(fresh=False):
    """Claude Code utilisable sans interaction (connexion au compte claude.ai, jeton ou clé
    d'API) : « claude auth status », gardé 30 s."""
    if not fresh and time.time() - _claude_status["at"] < 30:
        return _claude_status["value"]
    value = bool(os.environ.get("ANTHROPIC_API_KEY"))
    if not value and shutil.which("claude"):
        try:
            out = subprocess.run(["claude", "auth", "status", "--json"], capture_output=True, text=True, timeout=20).stdout
            value = bool(json.loads(out or "{}").get("loggedIn"))
        except (subprocess.SubprocessError, ValueError, OSError):
            home = os.path.expanduser("~")
            value = os.path.exists(os.path.join(home, ".claude", ".credentials.json"))
    _claude_status.update(at=time.time(), value=value)
    return value


# --- Mises à jour (page /updates/) ------------------------------------------------------
# Vérifiées au démarrage puis toutes les 6 h (scripts/update.py check) ; « Mettre à jour »
# remplace les fichiers du programme (tâche « update »), puis le serveur s'arrête : le
# conteneur redémarre tout seul (restart: unless-stopped) avec la nouvelle version.
# « Réinitialiser » (tâche « reset ») retélécharge la version installée et remet les fichiers
# du programme modifiés ou effacés, puis redémarre de la même façon.
update_state = {"checkedAt": 0.0, "result": None}


def update_check():
    try:
        out = subprocess.run(["python3", "scripts/update.py", "check"], cwd=ROOT, capture_output=True,
                             text=True, timeout=90).stdout
        result = json.loads(out)
    except (subprocess.SubprocessError, ValueError, OSError) as e:
        result = {"current": VERSION, "error": f"Vérification impossible ({e})."}
    update_state.update(checkedAt=time.time(), result=result)
    return result


def update_loop():
    time.sleep(60)  # laisse le démarrage se faire
    while True:
        update_check()
        time.sleep(6 * 3600)


def update_summary():
    r = update_state["result"] or {}
    return {"newer": bool(r.get("newer")), "latest": r.get("latest")}


# --- Connexion à Claude Code depuis l'interface (page /claude/) -----------------------
# « claude auth login » tourne dans un pseudo-terminal : il donne l'adresse de connexion
# (ouverte dans le navigateur de l'utilisateur, déjà connecté à claude.ai : il n'a qu'à
# autoriser), puis attend le code affiché après l'autorisation, que la page lui transmet.
login = {"proc": None, "fd": None, "output": "", "url": None}
login_lock = threading.Lock()
LOGIN_URL = re.compile(r"https://[^\s\x1b\x07\]]*oauth/authorize[^\s\x1b\x07\]]*")


def _login_reader(fd):
    while True:
        try:
            data = os.read(fd, 4096)
        except OSError:
            break
        if not data:
            break
        text = re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", data.decode("utf-8", "replace"))
        with login_lock:
            login["output"] = (login["output"] + text)[-20000:]
            m = LOGIN_URL.search(login["output"])
            if m and not login["url"]:
                login["url"] = m.group(0)


def login_stop():
    proc = login.get("proc")
    if proc and proc.poll() is None:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    if login.get("fd") is not None:
        try:
            os.close(login["fd"])
        except OSError:
            pass
    login.update(proc=None, fd=None)


def login_start(console=False):
    """Lance la connexion ; renvoie l'adresse à ouvrir (ou une erreur)."""
    import pty
    login_stop()
    master, slave = pty.openpty()
    env = dict(os.environ, BROWSER="true")  # pas de navigateur dans le conteneur
    proc = subprocess.Popen(["claude", "auth", "login"] + (["--console"] if console else []),
                            stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
    os.close(slave)
    login.update(proc=proc, fd=master, output="", url=None)
    threading.Thread(target=_login_reader, args=(master,), daemon=True).start()
    for _ in range(60):
        time.sleep(0.25)
        with login_lock:
            if login["url"]:
                return {"url": login["url"]}
        if proc.poll() is not None:
            break
    return {"error": "Claude Code n'a pas donné d'adresse de connexion.", "output": login["output"][-1500:]}


def login_code(code):
    """Transmet le code d'autorisation ; renvoie l'état de la connexion."""
    proc, fd = login.get("proc"), login.get("fd")
    if not proc or proc.poll() is not None or fd is None:
        return {"error": "La connexion a expiré : recommencez depuis « Se connecter »."}
    code = re.sub(r"[\r\n]", "", code).strip()  # une seule ligne : rien d'autre n'est tapé
    os.write(fd, code.encode() + b"\r")
    for _ in range(120):
        time.sleep(0.25)
        if proc.poll() is not None:
            break
    # Réussi seulement si « claude auth login » s'est terminé sans erreur (un mauvais code le
    # fait échouer ou attendre) et que Claude Code se dit connecté.
    ok = proc.poll() == 0 and claude_connected(fresh=True)
    out = login["output"][-1500:]
    login_stop()
    reason = next((l.strip() for l in reversed(out.splitlines()) if "fail" in l.lower() or "error" in l.lower()), "")
    return {"ok": True} if ok else {"error": "La connexion n'a pas abouti : vérifiez le code (il ne sert qu'une fois) et "
                                             "recommencez depuis « Ouvrir la page d'autorisation »."
                                             + (f" Détail : {reason[-200:]}" if reason else ""), "output": out}


def claude_account():
    """Compte connecté (« claude auth status --text »), pour la page /claude/."""
    if not shutil.which("claude"):
        return ""
    try:
        return subprocess.run(["claude", "auth", "status", "--text"], capture_output=True, text=True, timeout=20).stdout.strip()
    except (subprocess.SubprocessError, OSError):
        return ""


def instructions_info():
    if not os.path.exists(INSTRUCTIONS):
        return None
    text = open(INSTRUCTIONS, encoding="utf-8", errors="replace").read()
    return {"name": read_text(INSTRUCTIONS_NAME) or "instructions.md", "size": len(text.encode()),
            "preview": text[:600]}


def job_error(log, code):
    """Message d'erreur à montrer : la ligne « Error… » ou « …Error: » plutôt que la pile Node."""
    for line in reversed(log):
        s = line.strip()
        if s.startswith("Error") or "Error:" in s or s.startswith("ERREUR") or s.startswith("Erreur"):
            return s
    for line in reversed(log):
        if not line.strip().startswith("at "):
            return line.strip()
    return exit_reason(code, shell=True)


PROGRESS = re.compile(r"^@progression ([\d.e+-]+) ([\d.e+-]+) (.+)$")


def progress_of(line):
    """(libellé, fait, total) d'une ligne d'avancement (scripts/progress.py). None sinon."""
    m = PROGRESS.match(line)
    if m:
        return m.group(3), float(m.group(1)), float(m.group(2))
    return None


def media_seconds(path):
    """Durée d'une vidéo (ffmpeg -i), 0 si inconnue."""
    try:
        import imageio_ffmpeg
        info = subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-hide_banner", "-i", path],
                              capture_output=True, text=True, timeout=30).stderr
        h, m, s = re.search(r"Duration: (\d+):(\d+):([\d.]+)", info).groups()
        return int(h) * 3600 + int(m) * 60 + float(s)
    except Exception:  # noqa: BLE001 - estimation seulement
        return 0.0


def montage_seconds():
    """Durée du montage : celui de cette tâche s'il est déjà fait, sinon une estimation."""
    path = os.path.join(ROOT, "src", "data", "edit.json")
    try:
        # Seule la préparation refait le montage plus tard : sinon edit.json est le bon.
        if job["kind"] != "prepare" or os.path.getmtime(path) >= job["startedTs"]:
            edit = json.load(open(path))
            return edit["durationInFrames"] / edit["fps"]
    except (OSError, ValueError, KeyError, ZeroDivisionError):
        pass
    return 0.6 * job.get("rushSeconds", 0)  # montage générique : à peu près la parole du rush


def load_rates():
    rates = {k: v for k, (_, v) in RATES.items()}
    try:
        rates.update({k: float(v) for k, v in json.load(open(TIMINGS)).items()})
    except (OSError, ValueError, AttributeError):
        pass
    return rates


def estimate(key, rates):
    kind = RATES.get(key, ("fixe", 0))[0]
    scale = job.get("rushSeconds", 0) if kind == "rush" else montage_seconds() if kind == "montage" else 1
    return rates.get(key, 0) * scale


def learn_timings():
    """Recale les vitesses sur les durées mesurées de la tâche qui vient de réussir."""
    rates = load_rates()
    for key, (kind, _) in RATES.items():
        spent = job["spent"].get(key)
        scale = job.get("rushSeconds", 0) if kind == "rush" else montage_seconds() if kind == "montage" else 1
        if spent is None or scale <= 0:
            continue
        rates[key] = round(0.3 * rates.get(key, spent / scale) + 0.7 * spent / scale, 4)
    try:
        json.dump(rates, open(TIMINGS, "w"), indent=1)
    except OSError:
        pass


def end_phase(now):
    """Durée de l'opération en cours, comptée à son libellé (et retirée du reste de l'étape)."""
    p = job.get("progress")
    if p and not p.get("ended"):
        p["ended"] = True
        job["opStart"].setdefault(p["label"], p.get("start", 0.0))
        job["spent"][p["label"]] = job["spent"].get(p["label"], 0) + now - p["since"]
        job["stepPhases"] += now - p["since"]


def set_progress(found):
    label, done, total = found
    current = job.get("progress")
    now = time.time()
    if current and current["label"] != label:
        end_phase(now)
    same = current and current["label"] == label
    since = current["since"] if same else now
    # Part déjà faite quand l'opération commence (reprise après une pause) : la vitesse mesurée
    # ne compte que ce qui avance pendant cette tâche.
    start = current.get("start", 0.0) if same else min(1.0, done / total) if total else 0.0
    job["progress"] = {"label": label, "done": done, "total": total, "since": since, "at": now, "start": start}
    job["ops"][label] = {"done": done, "total": total, "unit": UNITS.get(label), "step": job["step"]}
    job["memOp"] = None  # mémoire comptée pour cette opération (sample_memory)


def speed_factor(rates):
    """Rapport durée réelle / prévue des opérations déjà finies de cette tâche (machine plus
    lente ou plus rapide que prévu) : il corrige aussi les opérations à venir."""
    real = planned = 0.0
    for key, spent in job["spent"].items():
        if key in RATES and RATES[key][0] != "fixe":
            real += spent
            planned += estimate(key, rates) * (1 - job["opStart"].get(key, 0.0))
    if planned < 1 or real < 1:
        return 1.0
    return min(10.0, max(0.1, real / planned))


def remaining_time():
    """(reste de l'opération en cours, reste de la tâche), en secondes."""
    rates = load_rates()
    k = speed_factor(rates)
    rates = {key: v * (k if RATES.get(key, ("fixe",))[0] != "fixe" else 1) for key, v in rates.items()}
    now = time.time()
    steps = job.get("steps") or []
    if job["step"] not in steps:
        return None, None
    i = steps.index(job["step"])
    plan = PLAN.get(job["step"], [])
    p = job.get("progress")
    current = None
    upcoming = plan
    if p and not p.get("ended"):
        f = min(1.0, p["done"] / p["total"]) if p["total"] else 0.0
        f0 = p.get("start", 0.0)
        model = estimate(p["label"], rates) * (1 - f)
        elapsed = now - p["since"]
        live = elapsed * (1 - f) / (f - f0) if f > f0 and elapsed > 3 else model
        w = min(1.0, (f - f0) / 0.25)  # vitesse mesurée de plus en plus suivie
        current = w * live + (1 - w) * model
        upcoming = plan[plan.index(p["label"]) + 1:] if p["label"] in plan else plan
    elif p and p["label"] in plan:
        upcoming = plan[plan.index(p["label"]) + 1:]
    rest = (current or 0) + sum(estimate(k, rates) for k in upcoming)
    # Reste de l'étape hors barres (installation de Whisper, découpage de l'audio, montage…).
    outside = now - job["stepStart"] - job["stepPhases"] - (now - p["since"] if p and not p.get("ended") else 0)
    rest += max(0.0, estimate(f"étape:{job['step']}", rates) - outside)
    for step in steps[i + 1:]:
        rest += sum(estimate(k, rates) for k in PLAN.get(step, [])) + estimate(f"étape:{step}", rates)
    return current, rest


def run_job(steps, arg, args, first=0):
    log = job.get("logFile")

    def write(text):
        if log:
            try:
                log.write(text + "\n")
            except (OSError, ValueError) as e:
                # Journal impossible à écrire (disque plein…) : dit une fois sur la page et dans
                # le journal du serveur (work/logs/demarrage.log), qui suit le reste.
                if not job.get("logError"):
                    job["logError"] = f"journal de la tâche non écrit ({e}) : disque plein ?"
                    print(f"Auto-montage : {job['logError']} ; {resources(LOGS)}", file=sys.stderr, flush=True)
                print(text, file=sys.stderr, flush=True)
    try:
        rush = os.path.join(RUSHES, arg) if job["kind"] == "prepare" else \
            os.path.join(ROOT, read_text(os.path.join(WORK, "rush.txt")) or "")
        # Plusieurs vidéos à assembler : le rush assemblé n'existe pas encore (ou va être refait).
        job["rushSeconds"] = sum(media_seconds(os.path.join(RUSHES, a)) for a in args) if len(args) > 1 \
            else style_seconds() if job["kind"] == "style" else media_seconds(rush)
        # Mémoire : besoin de la tâche (son étape la plus gourmande) et de chaque étape.
        paths = [os.path.join(RUSHES, a) for a in args] if job["kind"] == "prepare" else [rush]
        try:
            if not any(name in memoire.STEPS for name, _ in steps[first:]):
                raise LookupError  # tâche sans étape lourde (mise à jour, analyse du style…)
            job["memInfo"] = memoire.video_info(paths, montage=None if job["kind"] == "prepare" else montage_seconds())
            plan = memoire.summary([name for name, _ in steps[first:]], job["memInfo"])
            job["memory"] = {k: plan[k] for k in ("peak", "peakOp", "available", "total", "level", "message")}
            write(f"# Mémoire : environ {memoire.go(plan['peak'])} au plus fort ({plan['peakOp']}), "
                  f"{memoire.go(plan['available'] or 0)} libres sur {memoire.go(plan['total'] or 0)}"
                  + (f"\n# ATTENTION : {plan['message']}" if plan["message"] else ""))
        except LookupError:
            job["memInfo"] = None
        except Exception:  # noqa: BLE001 - estimation seulement
            traceback.print_exc()
            job["memInfo"] = None
        if job.get("resumed"):
            write(f"# Reprise à l'étape « {steps[first][0]} »")
        if job.get("reuse"):
            write("# Parties gardées d'une préparation abandonnée : " + ", ".join(kept_parts(args)))
        # Reprise, ou parties gardées par un abandon : ce qui est déjà fait n'est pas refait.
        env = dict(os.environ, AM_REPRISE="1") if job.get("resumed") or job.get("reuse") else None
        for index, (step, cmd) in enumerate(steps):
            if index < first:  # reprise : étapes déjà faites
                continue
            now = time.time()
            if job.get("stepStart"):
                end_phase(now)
                key = f"étape:{job['step']}"
                job["spent"][key] = job["spent"].get(key, 0) + now - job["stepStart"] - job["stepPhases"]
            job.update(step=step, stepIndex=index, progress=None, stepStart=now, stepPhases=0.0)
            save_task("running")
            write(f"\n## {datetime.datetime.now().strftime('%H:%M:%S')} Étape : {step} ({' '.join(cmd)})")
            check_memory(step, write)
            last_progress = 0.0
            if job.get("cancel"):
                raise JobPaused() if job.get("pause") else JobCancelled()
            # Groupe de processus à part : « Annuler » arrête le script et tout ce qu'il a lancé.
            argv = [a for c in cmd for a in (args if c == "{args}" else [c.replace("{arg}", arg)])]
            proc = subprocess.Popen(argv, cwd=ROOT, text=True, env=env,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, bufsize=1,
                                    start_new_session=True)
            job["proc"] = proc
            job.update(memOp=None, memPeaks={}, renderWorkers=None)
            sampler = threading.Thread(target=sample_memory, args=(proc, step), daemon=True)
            sampler.start()
            for line in proc.stdout:  # journal en direct (progression du rendu, étapes…)
                line = line.rstrip()
                if line.startswith("@memoire "):  # opération en cours, sans barre d'avancement
                    job["memOp"] = line[9:].strip()
                    continue
                nav = re.match(r"Navigateurs du rendu : (\d+)", line)
                if nav:  # nombre choisi d'après la mémoire libre : compte dans la mesure du rendu
                    job["renderWorkers"] = int(nav.group(1))
                found = progress_of(line) if line else None
                if found and job["kind"] == "final" and found[0] == "Rendu de la vidéo":
                    found = ("Rendu 4K",) + found[1:]
                if found:  # affiché en barre, pas au journal (un point d'avancement toutes les 30 s)
                    set_progress(found)
                    if time.time() - last_progress > 30:
                        last_progress = time.time()
                        write(f"   … {found[0]} : {found[1]:g} / {found[2]:g} ({resources(ROOT)})")
                        watch_memory(write)
                        save_task("running")  # avancement gardé si tout s'arrête d'un coup
                    continue
                if line:
                    job["log"] = (job["log"] + [line])[-200:]
                    write(line)
            code = proc.wait()
            job["proc"] = None
            sampler.join(timeout=5)
            if job.get("cancel"):
                raise JobPaused() if job.get("pause") else JobCancelled()
            write(f"## {datetime.datetime.now().strftime('%H:%M:%S')} Fin de l'étape ({exit_reason(code, shell=True)})")
            if code != 0:
                write(f"   {resources(ROOT)}")
                raise RuntimeError(job_error(job["log"], code))
            record_memory(write, partial=(job.get("resumed") or job.get("reuse")) and index == first)
        now = time.time()
        end_phase(now)
        key = f"étape:{job['step']}"
        job["spent"][key] = job["spent"].get(key, 0) + now - job["stepStart"] - job["stepPhases"]
        if not job.get("resumed") and not job.get("reuse"):  # durées d'une tâche reprise : pas représentatives
            learn_timings()
        remove_task()
        job.update(state="done", step="terminé", progress=None, finishedAt=datetime.datetime.now().isoformat())
        write(f"\n# RÉSULTAT : réussi ({round(time.time() - job['startedTs'])} s)")
        if job["kind"] in ("update", "reset"):  # programme remplacé : redémarrage du conteneur
            job["step"] = "redémarrage"
            threading.Timer(3, lambda: os._exit(0)).start()
    except JobPaused:
        remove_partial_files()
        job.update(state="paused", error=None, proc=None, finishedAt=datetime.datetime.now().isoformat())
        save_task("paused")
        job["progress"] = None
        write(f"\n# RÉSULTAT : pause (étape « {job['step']} » ; « Reprendre » la continue)")
    except JobCancelled:
        remove_partial_files()
        remove_task()
        job.update(state="cancelled", error=None, progress=None, proc=None,
                   finishedAt=datetime.datetime.now().isoformat())
        write(f"\n# RÉSULTAT : annulé (étape « {job['step']} »)")
    except Exception as e:  # noqa: BLE001 - l'erreur est rendue à la page
        if job.get("logError"):
            e = RuntimeError(f"{e} ({job['logError']})")
        job.update(state="error", error=str(e), proc=None, finishedAt=datetime.datetime.now().isoformat())
        if job["kind"] in RESUMABLE:  # reprise possible une fois le problème réglé (mémoire, place…)
            save_task("error")
        traceback.print_exc()
        write(f"\n{traceback.format_exc()}\n# RÉSULTAT : échec (étape « {job['step']} ») : {e}")
    finally:
        if log:
            log.close()


class JobCancelled(Exception):
    pass


class JobPaused(Exception):
    pass


# --- Pause et reprise (work/tache.json) -----------------------------------------------
def save_task(state):
    """Décrit la tâche en cours (reprenable) dans work/tache.json : de quoi la reprendre."""
    if job["kind"] not in RESUMABLE:
        return
    p = job.get("progress")
    elapsed = job.get("before", 0.0) + time.time() - job["startedTs"]
    rec = {"state": state, "kind": job["kind"], "arg": job["arg"], "args": job["args"], "steps": job["steps"],
           "index": job.get("stepIndex", 0), "step": job["step"], "ops": job["ops"], "elapsed": round(elapsed),
           "langue": job.get("langue"), "startedAt": job.get("firstStartedAt") or job["startedAt"],
           "error": job.get("error"), "at": datetime.datetime.now().isoformat(timespec="seconds"),
           "current": p["label"] if p else None}
    try:
        os.makedirs(WORK, exist_ok=True)
        json.dump(rec, open(TASK + ".part", "w"), ensure_ascii=False, indent=1)
        os.replace(TASK + ".part", TASK)
    except OSError:
        traceback.print_exc()


def remove_path(path):
    try:
        shutil.rmtree(path) if os.path.isdir(path) else os.remove(path)
    except OSError:
        pass


def remove_task():
    for path in (TASK, os.path.join(WORK, "reprise")):
        remove_path(path)


# « Abandonner » une préparation : chaque partie déjà faite peut être effacée ou gardée. Une
# partie gardée resservira à la prochaine préparation du même rush (work/reprise/garde.json :
# préparation lancée comme une reprise, AM_REPRISE=1, sans rien refaire de ce qui est gardé).
REPRISE = os.path.join(WORK, "reprise")
KEPT = os.path.join(REPRISE, "garde.json")
ABANDON_PARTS = [
    ("travail", "Version de travail", [os.path.join(RUSHES, "rush_1080.mp4"), os.path.join(REPRISE, "travail")]),
    ("son", "Son et transcription", [os.path.join(WORK, n) for n in ("rush_16k.wav", "vad.txt", "seg", "segments.json")]),
    ("visage", "Position du visage", [os.path.join(REPRISE, "visage.json")]),
    ("moments", "Extraits de la page des moments", [os.path.join(WORK, "moments", n) for n in ("clips", "thumbs")]),
]


def rush_fingerprint(args):
    """Vidéos d'un rush (nom, taille, date) : une partie gardée ne resert qu'au même rush."""
    out = []
    for a in args:
        try:
            st = os.stat(os.path.join(RUSHES, a))
            out.append([a, st.st_size, int(st.st_mtime)])
        except OSError:
            out.append([a, None, None])
    return out


def abandon_parts():
    """Parties déjà faites de la préparation en pause : [{id, label, size}] (celles qui existent)."""
    out = []
    for pid, label, paths in ABANDON_PARTS:
        size = sum(dir_size(p) if os.path.isdir(p) else os.path.getsize(p) for p in paths if os.path.exists(p))
        if any(os.path.exists(p) for p in paths):
            out.append({"id": pid, "label": label, "size": size})
    return out


def abandon_task(delete=None):
    """Abandonne la préparation en pause : efface les parties de « delete » (toutes sans liste),
    garde les autres pour la prochaine préparation du même rush."""
    rec = paused_task() or {}
    ids = [pid for pid, _, _ in ABANDON_PARTS]
    delete = ids if delete is None else [d for d in delete if d in ids]
    kept = []
    for pid, _, paths in ABANDON_PARTS:
        if pid in delete:
            for path in paths:
                remove_path(path)
        elif any(os.path.exists(p) for p in paths):
            kept.append(pid)
    remove_path(TASK)
    # Restes de reprise des parties effacées (la version de travail en cours : son.flac…).
    for name in os.listdir(REPRISE) if os.path.isdir(REPRISE) else []:
        if not (name == "travail" and "travail" in kept or name == "visage.json" and "visage" in kept):
            remove_path(os.path.join(REPRISE, name))
    if kept and rec.get("args"):
        os.makedirs(REPRISE, exist_ok=True)
        json.dump({"args": rec["args"], "rush": rush_fingerprint(rec["args"]), "kept": kept},
                  open(KEPT, "w"), ensure_ascii=False)
    return kept


def kept_parts(args):
    """Parties gardées par un abandon pour ces vidéos (même rush), ou []."""
    try:
        rec = json.load(open(KEPT))
        if rec.get("args") == list(args) and rec.get("rush") == rush_fingerprint(args):
            return rec.get("kept") or []
    except (OSError, ValueError, AttributeError):
        pass
    return []


def paused_task():
    """Tâche à reprendre (en pause, coupée par un arrêt, ou arrêtée par une erreur), ou None.
    Pendant qu'une tâche tourne, celle-ci n'en est pas une."""
    if job["state"] == "running":
        return None
    try:
        rec = json.load(open(TASK))
        if rec.get("kind") not in RESUMABLE or not isinstance(rec.get("steps"), list):
            return None
    except (OSError, ValueError, AttributeError):
        return None
    if rec.get("state") == "running":  # le serveur s'est arrêté pendant la tâche
        rec["state"] = "interrompu"
    return rec


def busy_error():
    """Message d'une tâche refusée : une autre tourne, ou une préparation attend d'être reprise."""
    rec = paused_task()
    if rec:
        return (f"La préparation de {rec['arg']} est en pause : reprenez-la ou abandonnez-la d'abord "
                "(page « Rushes et montages »).")
    return "Une tâche est en cours : attendez qu'elle finisse."


def busy():
    return job["state"] == "running" or paused_task() is not None


def resume_job():
    """Reprend la tâche en pause à l'étape où elle s'était arrêtée. Message d'erreur, ou None."""
    rec = paused_task()
    if not rec:
        return "Aucune tâche à reprendre."
    every = dict(PREPARE_WITH_INSTRUCTIONS)
    if not all(name in every for name in rec["steps"]):
        return "Cette tâche ne peut pas être reprise : abandonnez-la."
    missing = [a for a in rec["args"] if not os.path.isfile(os.path.join(RUSHES, a))]
    if missing:
        return f"La vidéo {missing[0]} n'est plus dans le dossier des rushes : la préparation ne peut pas reprendre."
    if "consignes de montage (Claude)" in rec["steps"][rec["index"]:] and not claude_connected():
        return "Claude Code n'est pas connecté : connectez-le (page « Connexion à Claude »), puis reprenez."
    if rec.get("langue") in LANGUAGES:  # langue de la transcription, lue par prepare.sh
        open(os.path.join(WORK, "langue.txt"), "w", encoding="utf-8").write(rec["langue"] + "\n")
    steps = [(name, every[name]) for name in rec["steps"]]
    if not start_job(rec["kind"], steps, rec["arg"], rec["args"], resume=rec):
        return busy_error()
    return None


# --- Mémoire (scripts/memoire.py) --------------------------------------------------------
def check_memory(step, write):
    """Au début de chaque étape : son besoin de mémoire comparé à la mémoire libre maintenant."""
    if not job.get("memInfo"):
        return
    try:
        if step in ("rendu de la version de travail", "rendu 1080p", "rendu 4K", "source 4K"):
            job["memInfo"]["montage"] = montage_seconds()  # montage fait entre-temps
        ops = memoire.operations([step], job["memInfo"])
        if not ops:
            return
        _, op, need = max(ops, key=lambda o: o[2])
        avail, total = memoire.available()
        level = memoire.verdict(need, avail)
        job["stepMemory"] = {"step": step, "op": op, "need": need, "available": avail, "level": level,
                             "message": memoire.advice(level, need, avail, f"l'étape « {op} »")}
        write(f"   Mémoire de l'étape : environ {memoire.go(need)} ({op}), {memoire.go(avail or 0)} libres"
              + ("" if level == "ok" else f" : {level.upper()}"))
    except Exception:  # noqa: BLE001 - estimation seulement
        traceback.print_exc()


def sample_memory(proc, step):
    """Pendant une étape : pic de mémoire de chaque opération (somme des PSS de l'étape et de tout
    ce qu'elle a lancé, une fois par seconde), dans job["memPeaks"]. L'opération est celle de la barre
    d'avancement en cours, ou d'une ligne « @memoire <opération> », ou la seule de l'étape."""
    alone = memoire.STEPS.get(step, [])
    while proc.poll() is None:
        used = memoire.tree_memory(proc.pid)
        p = job.get("progress")
        op = job.get("memOp") or (p["label"] if p and p["label"] in memoire.OPERATIONS else None) \
            or (alone[0] if len(alone) == 1 else None)
        if op in memoire.OPERATIONS and used:
            peaks = job["memPeaks"]
            peaks[op] = max(peaks.get(op, 0), used)
        time.sleep(1)


def record_memory(write, partial=False):
    """Étape réussie : pics mesurés gardés (work/memoire_mesures.json) pour les prochaines
    estimations. Étape reprise en route : pas gardés (une partie du travail était déjà faite)."""
    peaks, info = job.get("memPeaks") or {}, job.get("memInfo")
    if not peaks or not info:
        return
    write("   Mémoire mesurée : " + ", ".join(f"{op} {memoire.go(n)}" for op, n in peaks.items()))
    if partial:
        return
    for op, peak in peaks.items():
        try:
            w = job.get("renderWorkers")
            memoire.record(op, peak, {**info, "workers": w} if w and op in memoire.RENDERS else info)
        except Exception:  # noqa: BLE001 - mesure seulement
            traceback.print_exc()


def watch_memory(write):
    """Pendant une étape : mémoire presque épuisée (moins de 300 Mo ou de 4 % libres)."""
    avail, total = memoire.available()
    if avail is None:
        return
    low = avail < max(300 * memoire.MO, 0.04 * total)
    sm = job.get("stepMemory") or {}
    if low and not sm.get("low"):
        sm.update(low=True, level="insuffisant", available=avail,
                  message=f"La mémoire est presque épuisée ({memoire.go(avail)} libres) : l'étape risque d'être "
                          "arrêtée. Fermez d'autres programmes, ou mettez la préparation en pause et donnez plus "
                          "de mémoire à Docker (Réglages > Resources > Memory).")
        job["stepMemory"] = sm
        write(f"   ATTENTION : mémoire presque épuisée ({memoire.go(avail)} libres)")


# --- Journal (page /logs/) ------------------------------------------------------------
# Chaque tâche écrit son journal complet dans work/logs/<date>_<tâche>.log (les 40 derniers
# sont gardés) ; le démarrage du conteneur et le serveur écrivent dans work/logs/demarrage.log
# (docker/entrypoint.sh). La page « Journal » les affiche et en fait un rapport à envoyer.
LOGS = os.path.join(WORK, "logs")
KIND_NAMES = {"prepare": "Préparation", "regen": "Génération", "final": "Création 4K", "update": "Mise à jour",
              "reset": "Réinitialisation", "style": "Analyse du style"}


def open_job_log(kind, arg, steps):
    os.makedirs(LOGS, exist_ok=True)
    stamp = datetime.datetime.now().strftime("%Y-%m-%d_%H%M%S")
    path = os.path.join(LOGS, f"{stamp}_{kind}.log")
    f = open(path, "w", encoding="utf-8", buffering=1)
    f.write(f"# {KIND_NAMES.get(kind, kind)} : {arg}\n# Auto-montage {VERSION}, démarré le "
            f"{datetime.datetime.now().strftime('%d/%m/%Y à %H:%M:%S')}\n# Étapes : {', '.join(steps)}\n\n")
    logs = sorted(n for n in os.listdir(LOGS) if n[:4].isdigit() and n.endswith(".log"))
    for old in logs[:-40]:
        try:
            os.remove(os.path.join(LOGS, old))
        except OSError:
            pass
    return f


def close_interrupted_logs():
    """Au démarrage du serveur, aucune tâche ne tourne : un journal sans « # RÉSULTAT » est celui
    d'une tâche coupée par l'arrêt du serveur ou du conteneur. Il est complété pour le dire."""
    for name in sorted(os.listdir(LOGS))[-40:] if os.path.isdir(LOGS) else []:
        path = os.path.join(LOGS, name)
        if not (name[:4].isdigit() and name.endswith(".log")):
            continue
        try:
            with open(path, "rb") as f:
                f.seek(max(0, os.path.getsize(path) - 600))
                if re.search(rb"^# R\xc3\x89SULTAT : ", f.read(), re.M):
                    continue
            last = datetime.datetime.fromtimestamp(os.path.getmtime(path)).strftime("%d/%m/%Y à %H:%M:%S")
            with open(path, "a", encoding="utf-8") as f:
                f.write(f"\n# Dernière écriture le {last}, puis plus rien : le serveur s'est arrêté pendant la tâche "
                        "(Auto-montage ou Docker fermé, ordinateur éteint ou redémarré, mémoire de Docker épuisée…).\n"
                        f"# Serveur redémarré le {datetime.datetime.now().strftime('%d/%m/%Y à %H:%M:%S')} "
                        f"({resources(ROOT)}).\n# RÉSULTAT : interrompu\n")
        except OSError:
            pass


def log_list():
    out = []
    for name in sorted(os.listdir(LOGS), reverse=True) if os.path.isdir(LOGS) else []:
        path = os.path.join(LOGS, name)
        if not name.endswith(".log") or not os.path.isfile(path):
            continue
        state = "running"
        try:
            with open(path, "rb") as f:
                f.seek(max(0, os.path.getsize(path) - 600))
                tail = f.read().decode("utf-8", "replace")
            m = re.findall(r"^# RÉSULTAT : (\w+)", tail, re.M)
            state = m[-1] if m else ("running" if job["state"] == "running" and job.get("logName") == name else "interrompu")
        except OSError:
            pass
        kind = name[18:-4] if name[:4].isdigit() else name[:-4]
        other = {"demarrage": "Démarrage et serveur", "demarrage.prec": "Démarrage (journal précédent)",
                 "discussion": "Discussion avec Claude (erreurs)"}
        out.append({"name": name, "kind": kind if name[:4].isdigit() else "demarrage",
                    "label": KIND_NAMES.get(kind) or other.get(kind, kind),
                    "state": state if name[:4].isdigit() else "serveur", "size": os.path.getsize(path),
                    "modified": datetime.datetime.fromtimestamp(os.path.getmtime(path)).isoformat(timespec="seconds")})
    return out


def report_text():
    """Rapport à joindre à une demande d'aide : état du système, journaux récents."""
    lines = [f"Rapport Auto-montage {VERSION} — {datetime.datetime.now().strftime('%d/%m/%Y %H:%M:%S')}", ""]
    try:
        osr = read_text("/etc/os-release") or ""
        system = next((l.split("=", 1)[1].strip('"') for l in osr.splitlines() if l.startswith("PRETTY_NAME=")), "?")
    except Exception:  # noqa: BLE001
        system = "?"
    du = shutil.disk_usage(ROOT)
    lines += [f"Système du conteneur : {system} ; {os.cpu_count()} cœurs ; disque libre {du.free >> 30} Go",
              f"Claude Code : {'connecté' if claude_connected() else 'non connecté'}"
              f"{'' if shutil.which('claude') else ' (non installé)'}",
              f"Rush en cours : {read_text(os.path.join(WORK, 'rush.txt')) or 'aucun'} ; "
              f"page des moments : {'oui' if os.path.exists(os.path.join(MOMENTS, 'index.html')) else 'non'}",
              f"Tâche : {job['state']} {job.get('kind') or ''} {job.get('step') or ''} {job.get('error') or ''}", ""]
    logs = log_list()
    jobs = [l for l in logs if l["kind"] != "demarrage"][:3]
    for l in jobs:
        lines += ["=" * 78, f"{l['label']} ({l['name']}) : {l['state']}", "=" * 78,
                  open(os.path.join(LOGS, l["name"]), encoding="utf-8", errors="replace").read(), ""]
    start = os.path.join(LOGS, "demarrage.log")
    if os.path.exists(start):
        tail = open(start, encoding="utf-8", errors="replace").read().splitlines()[-300:]
        lines += ["=" * 78, "Démarrage et serveur (300 dernières lignes)", "=" * 78, *tail]
    return "\n".join(lines) + "\n"


def remove_partial_files(uploads=False):
    """Fichiers en cours d'écriture (*.part.*, renommés seulement une fois complets) d'une tâche
    annulée : les versions précédentes restent intactes. Avec uploads (démarrage du serveur, rien
    ne tourne) : aussi les envois coupés (.envoi-*) et les fichiers d'une tâche arrêtée en route."""
    for folder in (OUT, RUSHES):
        for name in os.listdir(folder) if os.path.isdir(folder) else []:
            if ".part." in name or (uploads and name.startswith(".envoi-")):
                try:
                    os.remove(os.path.join(folder, name))
                except OSError:
                    pass
    if uploads:  # images extraites par HyperFrames restées d'un rendu (place.extract_caches)
        for cache in place.extract_caches():
            shutil.rmtree(cache, ignore_errors=True)


def dir_size(path):
    total = 0
    for folder, _, names in os.walk(path):
        for name in names:
            try:
                total += os.lstat(os.path.join(folder, name)).st_size
            except OSError:
                pass
    return total


def no_space(need, folder):
    """Message « pas assez de place » : place libre, place nécessaire et ce qui en prend le plus
    dans Auto-montage (à supprimer pour en libérer)."""
    free = shutil.disk_usage(folder).free
    gb = lambda n: f"{n / 2**30:.1f} Go"  # noqa: E731
    parts = [(dir_size(RUSHES), "vidéos envoyées (montage/public/rushes)"),
             (dir_size(os.path.join(WORK, "projets")), "montages enregistrés (page Rushes et montages, « Supprimer »)"),
             (dir_size(OUT), "rendus (montage/out)"),
             (dir_size(os.path.join(WORK, "sauvegardes")), "sauvegardes (montage/work/sauvegardes)")]
    used = ", ".join(f"{label} {gb(n)}" for n, label in sorted(parts, reverse=True) if n >= 2**28)
    return (f"Pas assez de place sur le disque : il faut {gb(need)} libres, il en reste {gb(free)}."
            + (f" Dans Auto-montage : {used}." if used else "")
            + " Pour en libérer : « Libérer de la place », en bas de la page « Rushes et montages ».")


def cancel_job(pause=False):
    """Arrête la tâche en cours ; avec pause, elle est gardée pour être reprise (préparation)."""
    with job_lock:
        if job["state"] != "running" or (pause and job["kind"] not in RESUMABLE):
            return False
        job["cancel"] = True
        job["pause"] = pause
        proc = job.get("proc")
    if proc:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    return True


def start_job(kind, steps, arg, args=None, resume=None, reuse=False):
    """arg : objet de la tâche (rush, variante…) ; args : vidéos du rush (« {args} »), [arg] par défaut.
    resume : tâche en pause (work/tache.json) reprise à son étape. reuse : préparation qui reprend
    les parties gardées par un abandon (work/reprise/garde.json). Refusée si une tâche tourne ou
    si une préparation attend d'être reprise (sauf pour la reprendre)."""
    with job_lock:
        if job["state"] == "running" or (resume is None and kind not in ("update", "reset") and paused_task()):
            return False
        r = resume or {}
        job.update(state="running", kind=kind, step="démarrage", error=None, logError=None, log=[], arg=arg, progress=None,
                   cancel=False, pause=False, proc=None, logFile=None, logName=None,
                   spent={}, opStart={}, stepStart=None, stepPhases=0.0, rushSeconds=0.0,
                   steps=[name for name, _ in steps], args=args or [arg], stepIndex=r.get("index", 0),
                   ops=dict(r.get("ops") or {}), resumed=bool(resume), reuse=reuse, before=float(r.get("elapsed") or 0),
                   firstStartedAt=r.get("startedAt"), memory=None, stepMemory=None, memInfo=None,
                   langue=r.get("langue") or language(WORK),
                   startedAt=datetime.datetime.now().isoformat(), startedTs=time.time())
        try:
            title = arg + (f" ({' + '.join(args)})" if args and len(args) > 1 else "")
            job["logFile"] = open_job_log(kind, title + (" : reprise" if resume else ""), job["steps"])
            job["logName"] = os.path.basename(job["logFile"].name)
        except OSError:
            traceback.print_exc()
    threading.Thread(target=run_job, args=(steps, arg, args or [arg], r.get("index", 0)), daemon=True).start()
    return True


def job_status():
    status = {k: v for k, v in job.items() if k not in ("log", "spent", "proc", "cancel", "logFile", "opStart", "memInfo", "memPeaks", "memOp", "renderWorkers")} \
        | {"log": job["log"][-30:], "now": time.time(), "paused": paused_task(),
           "plan": PLAN}
    if job["state"] == "running":
        try:
            current, rest = remaining_time()
        except Exception:  # noqa: BLE001 - l'estimation ne doit jamais casser le suivi
            traceback.print_exc()
            current, rest = None, None
        elapsed = job.get("before", 0.0) + time.time() - job["startedTs"]
        status["remaining"] = None if rest is None else round(rest)
        status["overall"] = round(elapsed / (elapsed + rest), 4) if rest is not None and elapsed + rest > 0 else 0.0
        if status.get("progress") and current is not None:
            status["progress"] = status["progress"] | {"remaining": round(current)}
    else:
        status["overall"] = 1.0 if job["state"] == "done" else 0.0
    return status


# --- Discussion avec Claude Code (/chat/) -------------------------------------------------
# Chaque message lance « claude -p » depuis la racine du dépôt (CLAUDE.md chargé), en
# reprenant la conversation précédente (--resume). La réponse arrive en flux (stream-json) :
# textes et outils utilisés s'ajoutent au message en cours, que la page relit chaque seconde.
# Sans personne pour valider ses actions, Claude a des outils limités : lire et modifier les
# fichiers du projet, et lancer les seuls scripts de montage (pas de commande libre ni de web).
# Pour le reste : lanceur « Claude Code » (terminal, chaque action validée).
CHAT_DIR = os.path.join(WORK, "chat")
CHAT_HISTORY = os.path.join(CHAT_DIR, "history.json")
CHAT_SESSION = os.path.join(CHAT_DIR, "session.txt")
CHAT_SCRIPTS = ["python3 scripts/use_selection.py work/local_db *", "python3 scripts/build_edit.py",
                "bash scripts/render.sh apercu", "python3 scripts/make_moments.py",
                "python3 scripts/make_downloads.py",
                "python3 scripts/decoupage.py *",  # scinder ou fusionner des moments (puis make_moments)
                "python3 scripts/fonts_lib.py"]  # polices utilisables (lecture seule)
CHAT_TOOLS = ["Read", "Glob", "Grep", "Edit(montage/work/**)", "Write(montage/work/**)",
              *[f"Bash(cd montage && {c})" for c in CHAT_SCRIPTS]]
# Interdits même dans montage/work/ : la conversation elle-même (son numéro
# est passé à « claude --resume ») et les pages servies par l'interface (un texte piégé ne doit
# pas pouvoir y glisser du code). Même liste pour scripts/apply_instructions.sh et style_claude.sh.
CHAT_DENIED = [f"{t}({p})" for t in ("Edit", "Write") for p in
               ("montage/work/chat/**", "montage/work/**/*.html", "montage/work/**/*.js")]
SESSION_ID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
CHAT_CONTEXT = """Tu es dans l'interface locale d'Auto-montage (conteneur Docker ou Podman) ; l'utilisateur
te parle depuis la page http://localhost:8080/chat/ et ne voit que tes textes et le nom des outils
utilisés. Tu peux lire le projet, modifier les fichiers de montage/work/ (suggestions.json,
edit_choices.json, caption_style.json pour le style des sous-titres, local_db/…) et lancer seulement ces commandes, écrites exactement ainsi :
""" + "\n".join(f"cd montage && {c}" for c in CHAT_SCRIPTS) + """
(* : nom de la variante, par exemple travail).
Tout autre outil ou commande est refusé : dans ce cas, dis à l'utilisateur d'ouvrir le lanceur
« Claude Code » (terminal où il valide chaque action). Pas de page claude.ai à publier, pas
d'ArtifactData ni de routine : la page des moments (http://localhost:8080/) est servie depuis
montage/work/moments/, les choix de la page sont dans montage/work/local_db/, et son bouton
« Générer la version de travail » fait use_selection, build_edit, render.sh apercu et
make_moments. Tu ne peux pas poser de question à choix : demande en texte. Réponds en
français, simplement : l'utilisateur n'est pas développeur."""
chat = {"running": False, "messages": [], "proc": None}
chat_lock = threading.Lock()


def chat_load():
    try:
        chat["messages"] = json.load(open(CHAT_HISTORY, encoding="utf-8"))
    except (OSError, ValueError):
        chat["messages"] = []


def chat_save():
    os.makedirs(CHAT_DIR, exist_ok=True)
    tmp = CHAT_HISTORY + ".tmp"
    json.dump(chat["messages"][-300:], open(tmp, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    os.replace(tmp, CHAT_HISTORY)


def tool_detail(data):
    """Résumé court d'un appel d'outil : commande, fichier ou motif."""
    data = data if isinstance(data, dict) else {}
    for key in ("command", "file_path", "path", "pattern", "description"):
        if data.get(key):
            text = str(data[key]).replace(os.path.dirname(ROOT) + os.sep, "")
            return text if len(text) <= 160 else text[:157] + "…"
    return ""


def chat_event(reply, event):
    """Ajoute à la réponse en cours ce qu'apporte un événement stream-json de claude -p."""
    kind = event.get("type")
    if event.get("session_id"):
        os.makedirs(CHAT_DIR, exist_ok=True)
        open(CHAT_SESSION, "w").write(event["session_id"])
    if kind == "assistant":
        for block in (event.get("message") or {}).get("content") or []:
            if block.get("type") == "text" and block.get("text", "").strip():
                reply["parts"].append({"kind": "text", "text": block["text"].strip()})
            elif block.get("type") == "tool_use":
                reply["parts"].append({"kind": "tool", "name": block.get("name", "outil"),
                                       "detail": tool_detail(block.get("input"))})
    elif kind == "result":
        if event.get("is_error") or event.get("subtype", "success") != "success":
            reply["parts"].append({"kind": "error", "text": str(event.get("result") or event.get("subtype") or "erreur")})
        elif event.get("result") and not any(p["kind"] == "text" for p in reply["parts"]):
            reply["parts"].append({"kind": "text", "text": str(event["result"]).strip()})


def chat_log(text):
    """Erreurs de la discussion avec Claude, dans le journal (work/logs/discussion.log)."""
    try:
        os.makedirs(LOGS, exist_ok=True)
        with open(os.path.join(LOGS, "discussion.log"), "a", encoding="utf-8") as f:
            f.write(f"\n== {datetime.datetime.now():%d/%m/%Y %H:%M:%S}\n{text}\n")
    except OSError:
        pass


def run_chat(message, session):
    reply = chat["messages"][-1]
    if session and not SESSION_ID.fullmatch(session):
        session = ""  # numéro de conversation abîmé : nouvelle conversation (jamais une option)
    # Le message passe par l'entrée standard : un texte commençant par « - » n'est pas une option.
    cmd = ["claude", "-p", "--output-format", "stream-json", "--verbose",
           "--append-system-prompt", CHAT_CONTEXT, *(["--resume", session] if session else []),
           "--allowedTools", *CHAT_TOOLS, "--disallowedTools", *CHAT_DENIED]
    other = []  # lignes hors JSON : messages d'erreur de claude
    try:
        proc = subprocess.Popen(cmd, cwd=os.path.dirname(ROOT), text=True, bufsize=1,
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, start_new_session=True)
        chat["proc"] = proc
        proc.stdin.write(message)
        proc.stdin.close()
        for line in proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
            except ValueError:
                other = (other + [line])[-20:]
                continue
            if isinstance(event, dict):
                with chat_lock:
                    chat_event(reply, event)
        code = proc.wait()
        with chat_lock:
            if reply.get("stopped"):
                reply["parts"].append({"kind": "error", "text": "Arrêté."})
            elif code != 0 and not any(p["kind"] == "error" for p in reply["parts"]):
                text = "\n".join(other[-5:]) or f"Claude Code s'est arrêté (code {code})."
                if session and "No conversation found" in text and os.path.exists(CHAT_SESSION):
                    os.remove(CHAT_SESSION)  # conversation perdue (volume réinstallé) : repartir de zéro
                    text += "\nLa conversation précédente est introuvable : renvoyez votre message."
                reply["parts"].append({"kind": "error", "text": text})
                chat_log(f"Message : {message[:300]}\nCode {code}\n" + "\n".join(other))
            elif not reply["parts"]:
                reply["parts"].append({"kind": "error", "text": "Pas de réponse."})
    except Exception as e:  # noqa: BLE001 - l'erreur est rendue à la page
        with chat_lock:
            reply["parts"].append({"kind": "error", "text": str(e)})
        traceback.print_exc()
    finally:
        with chat_lock:
            reply["done"] = True
            chat.update(running=False, proc=None)
            chat_save()


def chat_send(message):
    with chat_lock:
        if chat["running"]:
            return "Claude répond encore : attendez la fin de sa réponse, ou arrêtez-la."
        now = datetime.datetime.now().isoformat(timespec="seconds")
        chat["messages"] += [{"role": "user", "text": message, "time": now},
                             {"role": "assistant", "parts": [], "time": now, "done": False}]
        chat["running"] = True
        chat_save()
    threading.Thread(target=run_chat, args=(message, read_text(CHAT_SESSION)), daemon=True).start()
    return None


def chat_stop():
    with chat_lock:
        proc = chat["proc"]
        if not proc:
            return
        chat["messages"][-1]["stopped"] = True
    try:
        os.killpg(proc.pid, signal.SIGTERM)  # claude et les scripts qu'il a lancés
    except ProcessLookupError:
        pass


def chat_reset():
    with chat_lock:
        if chat["running"]:
            return False
        chat["messages"] = []
        for path in (CHAT_SESSION, CHAT_HISTORY):
            if os.path.exists(path):
                os.remove(path)
    return True


def chat_status():
    with chat_lock:
        return {"running": chat["running"], "messages": chat["messages"][-200:],
                "claude": claude_connected(), "installed": bool(shutil.which("claude"))}


def run_project(*args):
    """scripts/project.py (montages enregistrés) : (réussi, message)."""
    proc = subprocess.run(["python3", "scripts/project.py", *args], cwd=ROOT, capture_output=True, text=True)
    return proc.returncode == 0, (proc.stdout + proc.stderr).strip().splitlines()[-1:] or [""]


def projects_data():
    proc = subprocess.run(["python3", "scripts/project.py", "list"], cwd=ROOT, capture_output=True, text=True)
    try:
        saved = json.loads(proc.stdout)
    except ValueError:
        saved = []
    current = os.path.basename(read_text(os.path.join(WORK, "rush.txt")) or "")
    thumbs = os.path.join(MOMENTS, "thumbs")
    names = sorted(n for n in os.listdir(thumbs) if n.startswith("m_")) if os.path.isdir(thumbs) else []
    return {"current": current or None, "currentPrepared": os.path.exists(os.path.join(MOMENTS, "index.html")),
            "currentThumb": f"/thumbs/{names[len(names) // 3]}" if names else None, "saved": saved}


def rush_list():
    current = os.path.basename(read_text(os.path.join(WORK, "rush.txt")) or "")
    items = []
    if os.path.isdir(RUSHES):
        for name in sorted(os.listdir(RUSHES), key=str.lower):
            path = os.path.join(RUSHES, name)
            if os.path.isfile(path) and name.lower().endswith(VIDEO_EXT) and not DERIVED.match(name) \
                    and not name.startswith(".") and ".part." not in name:
                items.append({"name": name, "size": os.path.getsize(path),
                              "modified": datetime.datetime.fromtimestamp(os.path.getmtime(path)).isoformat()})
    return {"rushes": items, "current": current or None, "prepared": os.path.exists(os.path.join(MOMENTS, "index.html")),
            "free": shutil.disk_usage(ROOT).free, "job": job_status(),
            "instructions": instructions_info(), "claude": claude_connected(), "style": style_info(),
            # Langue parlée (transcription) : choix de la prochaine préparation, langue du rush en cours.
            "langue": language(WORK), "langueRush": language(WORK, "langue_rush.txt") if current else None,
            "langues": LANGUAGES}


def safe_rush_name(name):
    """Nom de fichier de rush acceptable (pas de chemin, extension vidéo), ou None."""
    name = os.path.basename(name or "").strip()
    if not name or name.startswith((".", "-")) or DERIVED.match(name) or not name.lower().endswith(VIDEO_EXT):
        return None
    if not re.match(r"^[\w .()+,'&-]{1,200}$", name):
        return None
    return name


# --- Éléments de style (page « Rushes et montages ») ---------------------------------
# Sons personnels : bibliothèque commune à tous les montages, convertis en WAV dans
# public/sfx/perso/ ; ils rejoignent les effets de la page des moments (« perso-<nom> ») et
# Claude peut les placer (edit_choices.json, « sfx »). Vidéos d'exemple : work/style/videos/,
# avec une planche de 12 images (<nom>.planche.jpg) que Claude regarde pour s'en inspirer.
SOUNDS = os.path.join(ROOT, "public", "sfx", "perso")
STYLE_VIDEOS = os.path.join(WORK, "style", "videos")
AUDIO_EXT = (".wav", ".mp3", ".m4a", ".aac", ".ogg", ".oga", ".opus", ".flac", ".aif", ".aiff", ".webm", ".mp4")


def sound_id(name):
    stem = os.path.splitext(os.path.basename(name))[0]
    slug = re.sub(r"[^a-z0-9]+", "-", unicodedata.normalize("NFKD", stem).encode("ascii", "ignore").decode().lower()).strip("-")
    return slug[:40] or "son"


def style_info():
    sons = []
    for n in sorted(os.listdir(SOUNDS)) if os.path.isdir(SOUNDS) else []:
        if n.endswith(".wav"):
            sons.append({"id": f"perso-{n[:-4]}", "label": n[:-4].replace("-", " "),
                         "size": os.path.getsize(os.path.join(SOUNDS, n))})
    videos = []
    for n in sorted(os.listdir(STYLE_VIDEOS)) if os.path.isdir(STYLE_VIDEOS) else []:
        if n.lower().endswith(VIDEO_EXT) and not n.startswith("."):
            sheet = os.path.join(STYLE_VIDEOS, os.path.splitext(n)[0] + ".planche.jpg")
            videos.append({"name": n, "size": os.path.getsize(os.path.join(STYLE_VIDEOS, n)),
                           "sheet": os.path.exists(sheet)})
    return {"sons": sons, "videos": videos}


def make_contact_sheet(video):
    """Planche de 12 images réparties sur la vidéo (4 x 3, 360 px de large chacune)."""
    import imageio_ffmpeg
    duration = media_seconds(video) or 12
    out = os.path.splitext(video)[0] + ".planche.jpg"
    subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-v", "error", "-y", "-i", video,
                    "-vf", f"fps={12 / duration:.6f},scale=360:-2,tile=4x3", "-frames:v", "1", "-q:v", "4", out],
                   check=False, timeout=900)


# --- Téléchargements (sans découpe : les fichiers sont servis directement) -------
def downloads_data():
    rush = os.path.splitext(os.path.basename(read_text(os.path.join(WORK, "rush.txt")) or "rush"))[0]
    apercu = os.path.join(OUT, "montage_apercu.mp4")
    try:  # format du rush (9:16, 16:9, 4:3…)
        SIZE = edit_size(json.load(open(os.path.join(ROOT, "src", "data", "edit.json"))))
    except (OSError, ValueError):
        SIZE = edit_size(None)

    def res(w, h):  # « 720x1280 » en 9:16 : petit côté de w, au format du rush
        return res_label(*SIZE, w)
    sources = [
        ("720p", "Légère", res(720, 1280), os.path.join(MOMENTS, "montage.mp4"), "/montage.mp4",
         "Copie compressée de la version de travail, pour la partager ou la regarder sur téléphone."),
        ("1080p", "1080p", res(1080, 1920), apercu, "/out/montage_apercu.mp4",
         "Rendu de l'aperçu, à la résolution de la composition."),
        ("4k", "4K", res(2160, 3840), os.path.join(OUT, "montage_4k.mp4"), "/out/montage_4k.mp4",
         "Rendu final, pour la publication."),
    ]
    versions = []
    for vid, label, res, path, url, note in sources:
        if not os.path.exists(path):
            continue
        rendered = datetime.datetime.fromtimestamp(os.path.getmtime(path), datetime.timezone.utc)
        edit = os.path.join(ROOT, "src", "data", "edit.json")
        duration = json.load(open(edit))["durationInFrames"] / 30 if os.path.exists(edit) else 0
        versions.append({
            "id": vid, "label": label, "res": res, "note": note, "parts": [url], "reencoded": None,
            "filename": f"{rush}_{vid}_{rendered.astimezone().strftime('%Y-%m-%d_%Hh%M')}.mp4",
            "size": os.path.getsize(path), "duration": duration, "renderedAt": rendered.isoformat(),
            "stale": vid != "720p" and os.path.exists(apercu) and os.path.getmtime(path) < os.path.getmtime(apercu) - 60,
        })
    return {"rush": rush, "versions": versions, "momentsPage": "/", "final": final_info()}


def final_info():
    """Création de la version 4K depuis les pages : possible, et durée estimée sur cette machine."""
    apercu = os.path.join(OUT, "montage_apercu.mp4")
    try:
        edit = json.load(open(os.path.join(ROOT, "src", "data", "edit.json")))
        montage = edit["durationInFrames"] / edit["fps"]
        res = res_label(*edit_size(edit), 2160)
    except (OSError, ValueError, KeyError, ZeroDivisionError):
        montage, res = 0, "2160x3840"
    rates = load_rates()
    rush = media_seconds(os.path.join(ROOT, read_text(os.path.join(WORK, "rush.txt")) or "-"))
    scale = {"montage": montage, "rush": rush, "fixe": 1}
    estimate = sum(rates[k] * scale[RATES[k][0]] for k in ("Source 4K", "Rendu 4K", "étape:rendu 4K"))
    try:  # mémoire nécessaire (rendu 4K : le plus gourmand) comparée à la mémoire libre
        mem = memoire.summary([n for n, _ in FINAL_STEPS], memoire.video_info(
            [os.path.join(ROOT, read_text(os.path.join(WORK, "rush.txt")) or "-")], montage=montage))
        mem = {k: mem[k] for k in ("peak", "available", "level", "message")}
    except Exception:  # noqa: BLE001 - estimation seulement
        mem = None
    return {"available": os.path.exists(apercu) and montage > 0, "estimate": round(estimate), "res": res,
            "memory": mem, "running": job["state"] == "running" and job["kind"] == "final"}


# Noms sous lesquels l'interface est ouverte (navigateur de la machine, lanceurs, contrôle de
# santé du conteneur) ; d'autres avec AM_ALLOWED_HOSTS="nom1,nom2".
ALLOWED_HOSTS = {"localhost", "127.0.0.1", "[::1]",
                 *filter(None, os.environ.get("AM_ALLOWED_HOSTS", "").lower().replace(" ", "").split(","))}
JSON_MAX = 5 << 20  # corps JSON des requêtes : 5 Mo au plus


class Handler(BaseHTTPRequestHandler):
    server_version = "AutoMontage"

    def parse_request(self):
        """Refuse ce qui ne vient pas de l'interface elle-même :
        - un autre nom que localhost (DNS rebinding : site piégé dont le nom pointe vers
          127.0.0.1, qui pourrait sinon lire et piloter l'interface) ;
        - une écriture (POST, PUT, DELETE) envoyée par la page d'un autre site (CSRF)."""
        if not super().parse_request():
            return False
        host = (self.headers.get("Host") or "").strip().lower()
        name = host.rsplit(":", 1)[0] if not host.endswith("]") else host
        if name not in ALLOWED_HOSTS:
            self.send_error(403, "Adresse non autorisée : ouvrez http://localhost")
            return False
        if self.command not in ("GET", "HEAD"):
            origin = self.headers.get("Origin")
            fetch_site = self.headers.get("Sec-Fetch-Site")
            if (origin and urlparse(origin).netloc.lower() != host) or fetch_site in ("cross-site", "same-site"):
                self.send_error(403, "Requête d'un autre site refusée")
                return False
        return True

    def end_headers(self):
        # Pas d'affichage dans le cadre d'un autre site (clic piégé) ; types de fichiers tels quels.
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("X-Frame-Options", "DENY")
        self.send_header("Content-Security-Policy", "frame-ancestors 'none'")
        super().end_headers()

    def read_json(self):
        """Corps JSON de la requête (5 Mo au plus) ; ValueError s'il est invalide ou trop gros."""
        size = int(self.headers.get("Content-Length", 0) or 0)
        if not 0 <= size <= JSON_MAX:
            self.close_connection = True
            raise ValueError("corps trop gros")
        return json.loads(self.rfile.read(size) or b"{}")

    def log_message(self, fmt, *args):  # journal court : seulement les erreurs
        if args and str(args[1])[:1] in "45":
            super().log_message(fmt, *args)

    def send_json(self, data, status=200):
        body = json.dumps(data, ensure_ascii=False).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def send_html(self, html):
        body = html.encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def send_file(self, path):
        """Fichier statique, avec les requêtes partielles (Range) pour avancer dans les vidéos."""
        if not path or not os.path.isfile(path):
            return self.send_error(404)
        st = os.stat(path)
        size = st.st_size
        # Validateurs : le navigateur revalide sa copie et reçoit le nouveau fichier s'il a changé.
        etag = f'"{st.st_mtime_ns:x}-{size:x}"'
        modified = email.utils.formatdate(st.st_mtime, usegmt=True)
        if self.headers.get("If-None-Match") == etag:
            self.send_response(304)
            self.send_header("ETag", etag)
            return self.end_headers()
        start, end = 0, size - 1
        m = re.match(r"bytes=(\d*)-(\d*)$", self.headers.get("Range", ""))
        if_range = self.headers.get("If-Range")
        if if_range and if_range not in (etag, modified):
            m = None  # morceau d'une ancienne version : fichier entier
        if m and (m.group(1) or m.group(2)):
            if m.group(1):
                start = int(m.group(1))
                end = min(int(m.group(2)), size - 1) if m.group(2) else size - 1
            else:
                start = max(0, size - int(m.group(2)))
            if start > end:
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{size}")
                return self.end_headers()
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        else:
            self.send_response(200)
        self.send_header("Content-Type", mimetypes.guess_type(path)[0] or "application/octet-stream")
        self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("ETag", etag)
        self.send_header("Last-Modified", modified)
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        with open(path, "rb") as f:
            f.seek(start)
            left = end - start + 1
            while left > 0:
                chunk = f.read(min(1 << 20, left))
                if not chunk:
                    break
                try:
                    self.wfile.write(chunk)
                except (BrokenPipeError, ConnectionResetError):
                    return
                left -= len(chunk)

    def do_GET(self):
        path = unquote(urlparse(self.path).path)
        if path in ("/", "/index.html"):
            page = os.path.join(MOMENTS, "index.html")
            if not os.path.exists(page):  # pas encore de rush préparé
                return self.redirect("/rush/")
            return self.send_html(SKELETON + local_links(moments_page()))
        if path in ("/downloads", "/downloads/"):
            tpl = open(os.path.join(ROOT, "selection", "downloads.html"), encoding="utf-8").read()
            rush = downloads_data()["rush"]
            page = tpl.replace("__RUSH__", htmllib.escape(rush)).replace(
                "/*__DATA__*/null", json.dumps(downloads_data(), ensure_ascii=False).replace("<", "\\u003c"))
            return self.send_html(SKELETON + page)
        if path in ("/chat", "/chat/"):
            return self.send_html(SKELETON + open(os.path.join(ROOT, "local", "chat.html"), encoding="utf-8").read())
        if path == "/api/chat":
            return self.send_json(chat_status())
        if path in ("/rush", "/rush/"):
            return self.send_html(SKELETON + open(os.path.join(ROOT, "local", "rush.html"), encoding="utf-8").read())
        if path == "/api/version":
            return self.send_json({"version": VERSION, "started": STARTED})
        if path == "/api/instructions":
            return self.send_json({"text": open(INSTRUCTIONS, encoding="utf-8", errors="replace").read()
                                   if os.path.exists(INSTRUCTIONS) else "", "info": instructions_info()})
        if path in ("/updates", "/updates/"):
            return self.send_html(SKELETON + open(os.path.join(ROOT, "local", "updates.html"), encoding="utf-8").read())
        if path == "/api/update":
            if not update_state["result"]:
                update_check()
            return self.send_json(update_state | {"version": VERSION, "onDisk": disk_version()})
        if path in ("/claude", "/claude/"):
            return self.send_html(SKELETON + open(os.path.join(ROOT, "local", "claude.html"), encoding="utf-8").read())
        if path == "/api/claude":
            connected = claude_connected(fresh=True)
            return self.send_json({"connected": connected, "installed": bool(shutil.which("claude")),
                                   "account": claude_account() if connected else "",
                                   "apiKey": bool(os.environ.get("ANTHROPIC_API_KEY")),
                                   "pending": bool(login.get("proc") and login["proc"].poll() is None)})
        if path in ("/tutoriel", "/tutoriel/"):  # tutoriel (montré une fois au premier lancement)
            return self.send_html(SKELETON + open(os.path.join(ROOT, "local", "tutorial.html"), encoding="utf-8").read())
        if path.startswith("/tutoriel/"):  # captures et petits rendus du tutoriel
            return self.send_file(inside(os.path.join(ROOT, "local", "tutoriel"), path[len("/tutoriel/"):]))
        if path in ("/logs", "/logs/"):
            return self.send_html(SKELETON + open(os.path.join(ROOT, "local", "logs.html"), encoding="utf-8").read())
        if path == "/api/logs":
            return self.send_json({"logs": log_list(), "job": job_status()})
        if path == "/api/logs/rapport":
            body = report_text().encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Disposition",
                             f'attachment; filename="rapport-auto-montage-{datetime.datetime.now():%Y-%m-%d_%Hh%M}.txt"')
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            return self.wfile.write(body)
        lg = re.match(r"^/logs/([\w.-]+\.log)$", path)
        if lg:
            target = inside(LOGS, lg.group(1))
            if not target or not os.path.isfile(target):
                return self.send_error(404)
            body = open(target, "rb").read()
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            return self.wfile.write(body)
        if path == "/api/caption-style":
            return self.send_json(caption_style_info())
        m = re.match(r"^/api/fonts/([0-9a-f]{16})$", path)
        if m:
            path_ = font_files()[1].get(m.group(1))
            return self.send_file(path_) if path_ else self.send_error(404)
        if path == "/api/style/analyse":
            return self.send_json(style_analysis())
        if path.startswith("/style-analyse/"):  # planches de l'analyse
            return self.send_file(inside(STYLE_ANALYSE, path[len("/style-analyse/"):]))
        if path == "/api/habillage":
            return self.send_json(habillage())
        if path == "/api/son":
            analyse = os.path.join(WORK, "son_analyse.json")
            return self.send_json({"settings": sound_settings(),
                                   "analyse": json.load(open(analyse)) if os.path.exists(analyse) else None})
        if path == "/api/effects":
            return self.send_json(effects_info())
        if path == "/api/status":
            return self.send_json({"version": VERSION, "claude": claude_connected(),
                                   "claudeInstalled": bool(shutil.which("claude")),
                                   "styleAnalysis": read_text(os.path.join(WORK, "analyse_style.md")),
                                   "update": update_summary(),
                                   "tutorialSeen": os.path.exists(TUTORIAL_SEEN)})
        if path.startswith("/sfx-defaut/"):  # sons fournis (écoute sur la page « Rushes et montages »)
            return self.send_file(inside(os.path.join(ROOT, "public", "sfx"), os.path.basename(path)))
        if path.startswith("/sfx/perso/"):  # sons personnels (écoute sur la page « Rushes et montages »)
            return self.send_file(inside(SOUNDS, path[len("/sfx/perso/"):]))
        if path.startswith("/style/"):  # planches des vidéos d'exemple
            return self.send_file(inside(STYLE_VIDEOS, path[len("/style/"):]))
        if path == "/api/projects":
            return self.send_json(projects_data())
        m = re.match(r"^/api/projects/([^/]+)/thumb$", path)
        if m and SAFE_ID.match(m.group(1)):
            folder = inside(os.path.join(WORK, "projets"), os.path.join(m.group(1), "work", "moments", "thumbs"))
            names = sorted(n for n in os.listdir(folder) if n.startswith("m_")) if folder and os.path.isdir(folder) else []
            return self.send_file(os.path.join(folder, names[len(names) // 3]) if names else None)
        if path == "/api/downloads":
            return self.send_json(downloads_data())
        if path == "/api/place":
            return self.send_json(place.inventory())
        if path == "/api/rushes":
            return self.send_json(rush_list())
        if path == "/api/memoire":  # mémoire nécessaire à la préparation d'un rush (avant de la lancer)
            q = parse_qs(urlparse(self.path).query)
            parts = [safe_rush_name(r) for r in q.get("rush", [])[:20]]
            if not parts or not all(parts) or not all(os.path.isfile(os.path.join(RUSHES, n)) for n in parts):
                return self.send_json({"error": "Rush introuvable dans public/rushes."}, 400)
            steps = [n for n, _ in (PREPARE_WITH_INSTRUCTIONS if q.get("instructions") == ["1"] else PREPARE_STEPS)]
            plan = memoire.summary(steps, memoire.video_info([os.path.join(RUSHES, n) for n in parts]))
            kept = kept_parts(parts)
            plan["kept"] = [label for pid, label, _ in ABANDON_PARTS if pid in kept]
            return self.send_json(plan)
        if path == "/api/job/parts":  # parties déjà faites d'une préparation en pause (« Abandonner »)
            return self.send_json({"parts": abandon_parts() if paused_task() else []})
        if path in ("/local/shim.js", "/local/menu.js"):
            return self.send_file(os.path.join(ROOT, "local", os.path.basename(path)))
        if path.startswith("/out/"):
            return self.send_file(inside(OUT, path[len("/out/"):]))
        if path in ("/api/regen", "/api/job"):
            return self.send_json(job_status())
        m = re.match(r"^/api/db/([^/]+)$", path)
        if m:
            coll = m.group(1)
            if not SAFE_ID.match(coll):
                return self.send_json({"error": "collection invalide"}, 400)
            docs = []
            folder = os.path.join(DB, coll)
            if os.path.isdir(folder):
                for name in sorted(os.listdir(folder)):
                    if name.endswith(".json"):
                        try:
                            docs.append({"id": name[:-5], "data": json.load(open(os.path.join(folder, name)))})
                        except ValueError:
                            pass
            return self.send_json({"docs": docs})
        # Le reste : fichiers de la page des moments (extraits, vignettes, sons, police, montage).
        return self.send_file(inside(MOMENTS, path.lstrip("/")))

    def redirect(self, location):
        self.send_response(302)
        self.send_header("Location", location)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def upload_rush(self, raw_name):
        """Envoi d'un rush depuis la page (sélecteur de fichiers du navigateur), par morceaux."""
        name = safe_rush_name(raw_name)
        if not name:
            return self.send_json({"error": "Nom de fichier refusé : choisissez une vidéo (.mov, .mp4…)."}, 400)
        try:
            size = int(self.headers.get("Content-Length", ""))
        except ValueError:
            return self.send_json({"error": "Taille du fichier inconnue."}, 411)
        os.makedirs(RUSHES, exist_ok=True)
        if shutil.disk_usage(RUSHES).free < size + (2 << 30):
            return self.send_json({"error": no_space(size + (2 << 30), RUSHES)}, 507)
        tmp = os.path.join(RUSHES, f".envoi-{name}")
        left = size
        try:
            with open(tmp, "wb") as f:
                while left > 0:
                    chunk = self.rfile.read(min(4 << 20, left))
                    if not chunk:
                        break
                    f.write(chunk)
                    left -= len(chunk)
        except OSError:  # page fermée pendant l'envoi, disque plein : pas de reste caché de plusieurs Go
            left = left or 1
        if left:
            if os.path.exists(tmp):
                os.remove(tmp)
            return self.send_json({"error": "Envoi interrompu."}, 400)
        os.replace(tmp, os.path.join(RUSHES, name))
        return self.send_json({"ok": True, "name": name})

    def receive(self, dest, max_size=None):
        """Corps de la requête écrit dans dest (par morceaux, via un fichier provisoire)."""
        try:
            size = int(self.headers.get("Content-Length", ""))
        except ValueError:
            return "Taille du fichier inconnue."
        if max_size and size > max_size:
            return f"Fichier trop lourd (au plus {max_size >> 20} Mo)."
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        if shutil.disk_usage(os.path.dirname(dest)).free < size + (1 << 30):
            return no_space(size + (1 << 30), os.path.dirname(dest))
        tmp = os.path.join(os.path.dirname(dest), f".envoi-{os.path.basename(dest)}")
        left = size
        try:
            with open(tmp, "wb") as f:
                while left > 0:
                    chunk = self.rfile.read(min(4 << 20, left))
                    if not chunk:
                        break
                    f.write(chunk)
                    left -= len(chunk)
        except OSError:
            left = left or 1
        if left:
            if os.path.exists(tmp):
                os.remove(tmp)
            return "Envoi interrompu."
        os.replace(tmp, dest)
        return None

    def upload_style(self, kind, raw_name):
        name = os.path.basename(raw_name).strip()
        if not name or name.startswith(".") or not re.match(r"^[\w .()+,'&-]{1,200}$", name):
            return self.send_json({"error": "Nom de fichier refusé."}, 400)
        if kind == "sons":
            if not name.lower().endswith(AUDIO_EXT):
                return self.send_json({"error": "Choisissez un fichier son (.wav, .mp3, .m4a…)."}, 400)
            tmp = os.path.join(SOUNDS, ".source-" + name)
            error = self.receive(tmp, 100 << 20)
            if error:
                return self.send_json({"error": error}, 400)
            import imageio_ffmpeg
            sid = sound_id(name)
            proc = subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-v", "error", "-y", "-i", tmp, "-vn", "-t", "30",
                                   "-ac", "2", "-ar", "48000", os.path.join(SOUNDS, sid + ".wav")],
                                  capture_output=True, text=True)
            os.remove(tmp)
            if proc.returncode:
                return self.send_json({"error": "Ce fichier n'a pas pu être lu comme un son."}, 400)
            if os.path.isdir(MOMENTS):  # écoute sur la page des moments déjà ouverte
                os.makedirs(os.path.join(MOMENTS, "sfx"), exist_ok=True)
                subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-v", "error", "-y", "-i", os.path.join(SOUNDS, sid + ".wav"),
                                "-c:a", "libmp3lame", "-b:a", "96k", os.path.join(MOMENTS, "sfx", f"perso-{sid}.mp3")], check=False)
            return self.send_json({"ok": True, "id": f"perso-{sid}", "label": sid.replace("-", " ").capitalize(),
                                   "style": style_info()})
        if kind == "polices":  # police personnelle pour les sous-titres
            if not name.lower().endswith(fonts_lib.FONT_EXT):
                return self.send_json({"error": "Choisissez un fichier de police (.ttf, .otf, .woff, .woff2)."}, 400)
            os.makedirs(fonts_lib.PERSO, exist_ok=True)
            error = self.receive(os.path.join(fonts_lib.PERSO, name), 30 << 20)
            if error:
                return self.send_json({"error": error}, 400)
            return self.send_json(caption_style_info())
        if not name.lower().endswith(VIDEO_EXT):
            return self.send_json({"error": "Choisissez une vidéo (.mp4, .mov…)."}, 400)
        dest = os.path.join(STYLE_VIDEOS, name)
        error = self.receive(dest, 4 << 30)
        if error:
            return self.send_json({"error": error}, 400)
        threading.Thread(target=make_contact_sheet, args=(dest,), daemon=True).start()
        return self.send_json({"ok": True, "style": style_info()})

    def upload_instructions(self):
        """Consignes de montage (texte ou Markdown, 1 Mo au plus) -> work/instructions.md."""
        name = os.path.basename(unquote(self.headers.get("X-File-Name", "")) or "instructions.md")
        size = int(self.headers.get("Content-Length", "0") or 0)
        if not name.lower().endswith((".md", ".txt", ".markdown")) or not 0 < size <= (1 << 20):
            return self.send_json({"error": "Choisissez un fichier texte (.md ou .txt) de moins de 1 Mo."}, 400)
        raw = self.rfile.read(size)
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            text = raw.decode("latin-1")
        os.makedirs(WORK, exist_ok=True)
        open(INSTRUCTIONS, "w", encoding="utf-8").write(text)
        open(INSTRUCTIONS_NAME, "w", encoding="utf-8").write(name)
        return self.send_json({"ok": True, "instructions": instructions_info()})

    def do_DELETE(self):
        st = re.match(r"^/api/style/(sons|videos|polices)/(.+)$", unquote(urlparse(self.path).path))
        if st:
            kind, name = st.group(1), os.path.basename(st.group(2))
            if kind == "sons":
                paths = [inside(SOUNDS, name.removeprefix("perso-") + ".wav")]
            elif kind == "polices":
                paths = [inside(fonts_lib.PERSO, name)]
            else:
                paths = [inside(STYLE_VIDEOS, name), inside(STYLE_VIDEOS, os.path.splitext(name)[0] + ".planche.jpg")]
            for path in paths:
                if path and os.path.isfile(path):
                    os.remove(path)
            if kind == "polices":
                return self.send_json(caption_style_info())
            return self.send_json({"ok": True, "style": style_info()})
        if urlparse(self.path).path != "/api/instructions":
            return self.send_error(404)
        for path in (INSTRUCTIONS, INSTRUCTIONS_NAME):
            if os.path.exists(path):
                os.remove(path)
        return self.send_json({"ok": True})

    def do_PUT(self):
        if urlparse(self.path).path == "/api/instructions":
            return self.upload_instructions()
        if urlparse(self.path).path in ("/api/caption-style", "/api/effects", "/api/son", "/api/habillage"):
            try:
                body = self.read_json()
            except ValueError:
                return self.send_json({"error": "JSON invalide"}, 400)
            if not isinstance(body, dict):
                return self.send_json({"error": "JSON invalide"}, 400)
            os.makedirs(WORK, exist_ok=True)
            if urlparse(self.path).path == "/api/habillage":
                h = save_habillage(body)
                return self.send_json(h) if h is not None else self.send_json({"error": "Réglage invalide"}, 400)
            if urlparse(self.path).path == "/api/son":
                s = save_sound(body)
                return self.send_json({"settings": s}) if s is not None else self.send_json({"error": "Réglage invalide"}, 400)
            if urlparse(self.path).path == "/api/effects":
                ok = lambda items: sorted({h for h in items or [] if isinstance(h, str) and SAFE_ID.match(h)})
                # Seules les clés envoyées changent (effets écartés, à décocher, volume général).
                hidden = ok(body["hidden"]) if "hidden" in body else effects_hidden()
                purge = ok(body["purge"]) if "purge" in body else effects_file("purge")
                try:
                    volume = max(0.0, min(2.0, float(body["volume"]))) if "volume" in body else effects_volume()
                except (TypeError, ValueError):
                    volume = effects_volume()
                json.dump({"hidden": hidden, "purge": purge, "volume": round(volume, 2), "defauts": DEFAULTS_VERSION},
                          open(EFFECTS_FILE, "w"), indent=1)
                return self.send_json(effects_info())
            fonts_lib.save_style(body)
            return self.send_json(caption_style_info())
        st = re.match(r"^/api/style/(sons|videos|polices)/(.+)$", unquote(urlparse(self.path).path))
        if st:
            return self.upload_style(st.group(1), st.group(2))
        up = re.match(r"^/api/rushes/(.+)$", unquote(urlparse(self.path).path))
        if up:
            return self.upload_rush(up.group(1))
        m = re.match(r"^/api/db/([^/]+)/([^/]+)$", unquote(urlparse(self.path).path))
        if not m or not SAFE_ID.match(m.group(1)) or not SAFE_ID.match(m.group(2)):
            return self.send_json({"error": "document invalide"}, 400)
        try:
            data = self.read_json()
        except ValueError:
            return self.send_json({"error": "JSON invalide"}, 400)
        folder = os.path.join(DB, m.group(1))
        os.makedirs(folder, exist_ok=True)
        tmp = os.path.join(folder, f".{m.group(2)}.tmp")
        json.dump(data, open(tmp, "w"), ensure_ascii=False, indent=1)
        os.replace(tmp, os.path.join(folder, f"{m.group(2)}.json"))
        return self.send_json({"ok": True})

    def do_POST(self):
        route = urlparse(self.path).path
        if route not in ("/api/regen", "/api/prepare", "/api/final", "/api/chat", "/api/chat/stop", "/api/chat/new",
                         "/api/projects/open", "/api/projects/delete", "/api/projects/save", "/api/job/cancel", "/api/job/pause",
                         "/api/job/resume", "/api/job/abandon", "/api/place/delete",
                         "/api/claude/login", "/api/claude/code", "/api/claude/logout",
                         "/api/update/check", "/api/update/apply", "/api/update/reset", "/api/restart", "/api/style/analyse", "/api/son/analyse", "/api/tutorial/seen",
                         "/api/moments/merge", "/api/moments/split", "/api/moments/unmerge", "/api/moments/unsplit"):
            return self.send_error(404)
        try:
            body = self.read_json()
        except ValueError:
            body = {}
        if route == "/api/update/check":
            return self.send_json(update_check() | {"version": VERSION, "onDisk": disk_version()})
        if route == "/api/restart":  # fichiers changés sur le disque : le conteneur redémarre tout seul
            if job and job.get("state") == "running":
                return self.send_json({"error": "Une tâche est en cours : attendez qu'elle finisse."}, 409)
            threading.Timer(1, lambda: os._exit(0)).start()
            return self.send_json({"ok": True}, 202)
        if route == "/api/update/apply":
            r = update_check()
            if r.get("error") or not r.get("newer"):
                if not r.get("error") and disk_version() != VERSION:
                    return self.send_json({"error": f"Les fichiers d'Auto-montage sont déjà en version {disk_version()}, mais "
                                                    f"il tourne encore en {VERSION} : redémarrez-le.", "restart": True}, 400)
                return self.send_json({"error": r.get("error") or "Vous avez déjà la dernière version."}, 400)
            if not start_job("update", [("mise à jour", ["python3", "scripts/update.py", "apply", r["tag"]])], r["tag"]):
                return self.send_json({"error": busy_error()}, 409)
            return self.send_json({"ok": True, "tag": r["tag"]}, 202)
        if route == "/api/update/reset":
            if body.get("confirm") is not True:  # la page demande toujours une confirmation avant
                return self.send_json({"error": "Réinitialisation non confirmée."}, 400)
            tag = f"v{VERSION}"  # version qui tourne (VERSION sur le disque a pu être modifié)
            if not start_job("reset", [("réinitialisation", ["python3", "scripts/update.py", "reset", tag])], tag):
                return self.send_json({"error": busy_error()}, 409)
            return self.send_json({"ok": True, "tag": tag}, 202)
        if route == "/api/claude/login":
            if not shutil.which("claude"):
                return self.send_json({"error": "Claude Code n'est pas installé dans ce conteneur."}, 400)
            out = login_start(console=bool(body.get("console")))
            return self.send_json(out, 400 if "error" in out else 200)
        if route == "/api/claude/code":
            code = str(body.get("code") or "").strip()
            if not code or len(code) > 500:
                return self.send_json({"error": "Collez le code affiché après l'autorisation."}, 400)
            out = login_code(code)
            return self.send_json(out, 400 if "error" in out else 200)
        if route == "/api/claude/logout":
            subprocess.run(["claude", "auth", "logout"], capture_output=True, timeout=30)
            claude_connected(fresh=True)
            return self.send_json({"ok": True})
        if route == "/api/job/cancel":
            if not cancel_job():
                return self.send_json({"error": "Aucune tâche en cours."}, 409)
            return self.send_json({"ok": True})
        if route == "/api/job/pause":
            if not cancel_job(pause=True):
                return self.send_json({"error": "Aucune préparation en cours."}, 409)
            return self.send_json({"ok": True})
        if route == "/api/job/resume":
            error = resume_job()
            return self.send_json({"error": error}, 409) if error else self.send_json({"ok": True}, 202)
        if route == "/api/job/abandon":
            delete = body.get("delete")  # parties à effacer (toutes sans liste)
            if delete is not None and (not isinstance(delete, list) or not all(isinstance(d, str) for d in delete)):
                return self.send_json({"error": "Choix invalide."}, 400)
            with job_lock:
                if job["state"] == "running" or not paused_task():
                    return self.send_json({"error": "Aucune préparation en pause."}, 409)
                kept = abandon_task(delete)
            return self.send_json({"ok": True, "kept": kept})
        if route.startswith("/api/projects/"):
            action = route.rsplit("/", 1)[1]
            pid = str(body.get("id") or "")
            if action != "save" and not SAFE_ID.match(pid):
                return self.send_json({"error": "montage invalide"}, 400)
            with job_lock:  # pas pendant une préparation, une génération ou une création 4K
                if busy():
                    return self.send_json({"error": busy_error()}, 409)
                ok, msg = run_project(action, *([pid] if action != "save" else []))
            return self.send_json({"ok": ok, "message": msg[0]} if ok else {"error": msg[0]}, 200 if ok else 400)
        if route == "/api/place/delete":
            ids = body.get("ids")
            if not isinstance(ids, list) or not ids or not all(isinstance(i, str) for i in ids):
                return self.send_json({"error": "Rien de choisi."}, 400)
            with job_lock:  # une tâche en cours écrit dans ces dossiers (et ses fichiers .part.*)
                if job["state"] == "running":
                    return self.send_json({"error": "Une tâche est en cours : attendez qu'elle finisse."}, 409)
                rec = paused_task()  # vidéos d'une préparation en pause : gardées pour la reprendre
                if rec and any(i == f"rush:{a}" for i in ids for a in rec["args"]):
                    return self.send_json({"error": f"La préparation de {rec['arg']} est en pause : ses vidéos sont "
                                                    "gardées tant qu'elle n'est pas reprise ou abandonnée."}, 409)
                before = shutil.disk_usage(ROOT).free
                try:
                    done = place.delete(ids)
                except OSError as e:
                    return self.send_json({"error": f"Suppression incomplète : {e}"}, 500)
            return self.send_json({"ok": True, "deleted": done, "freed": max(0, shutil.disk_usage(ROOT).free - before)})
        if route == "/api/final":
            if not final_info()["available"]:
                return self.send_json({"error": "Pas encore de version de travail : générez-la d'abord."}, 400)
            if not start_job("final", FINAL_STEPS, "4k"):
                return self.send_json({"error": busy_error()}, 409)
            return self.send_json({"ok": True}, 202)
        if route == "/api/chat":
            message = str(body.get("message") or "").strip()
            if not message:
                return self.send_json({"error": "Message vide."}, 400)
            if not shutil.which("claude"):
                return self.send_json({"error": "Claude Code n'est pas installé dans ce conteneur."}, 400)
            if not claude_connected():
                return self.send_json({"error": "Claude Code n'est pas connecté : ouvrez-le une fois (lanceur « Claude Code »)."}, 400)
            error = chat_send(message[:20000])
            return self.send_json({"error": error}, 409) if error else self.send_json({"ok": True}, 202)
        if route == "/api/chat/stop":
            chat_stop()
            return self.send_json({"ok": True})
        if route == "/api/chat/new":
            if not chat_reset():
                return self.send_json({"error": "Arrêtez d'abord la réponse en cours."}, 409)
            return self.send_json({"ok": True})
        if route.startswith("/api/moments/"):
            # Scinder ou fusionner des moments (scripts/decoupage.py), puis refaire la page des
            # moments (seuls les extraits touchés sont recalculés).
            if busy():
                return self.send_json({"error": busy_error()}, 409)
            action = route.rsplit("/", 1)[1]
            try:
                mid = int(body.get("id"))
                extra = [float(body.get("t", 0))] if action == "split" else []
                message = getattr(decoupage, action)(mid, *extra)
            except decoupage.Refus as e:
                return self.send_json({"error": str(e)}, 400)
            except (TypeError, ValueError):
                return self.send_json({"error": "Moment invalide."}, 400)
            if not start_job("decoupage", [("page des moments", ["python3", "scripts/make_moments.py"])], message):
                return self.send_json({"error": busy_error()}, 409)
            return self.send_json({"ok": True, "message": message}, 202)
        if route == "/api/tutorial/seen":
            os.makedirs(WORK, exist_ok=True)
            open(TUTORIAL_SEEN, "w").write(datetime.datetime.now().isoformat(timespec="seconds") + "\n")
            return self.send_json({"ok": True})
        if route == "/api/son/analyse":  # quelques secondes : faite tout de suite
            if not os.path.exists(os.path.join(ROOT, "src", "data", "edit.json")) or not read_text(os.path.join(WORK, "rush.txt")):
                return self.send_json({"error": "Pas encore de version de travail."}, 400)
            proc = subprocess.run(["python3", "scripts/analyze_sound.py"], cwd=ROOT, capture_output=True, text=True, timeout=600)
            if proc.returncode:
                return self.send_json({"error": (proc.stderr.strip().splitlines() or ["échec"])[-1]}, 500)
            return self.send_json({"settings": sound_settings(), "analyse": json.load(open(os.path.join(WORK, "son_analyse.json")))})
        if route == "/api/style/analyse":
            if not style_videos():
                return self.send_json({"error": "Ajoutez d'abord des vidéos d'exemple."}, 400)
            if not claude_connected():
                return self.send_json({"error": "Claude Code n'est pas connecté : connectez-le sur la page « Connexion à Claude »."}, 400)
            if not start_job("style", STYLE_STEPS, f"{len(style_videos())} vidéo(s) d'exemple"):
                return self.send_json({"error": busy_error()}, 409)
            return self.send_json({"ok": True}, 202)
        if route == "/api/prepare":
            # Une vidéo (« rush ») ou plusieurs (« rushes », dans l'ordre) : assemblées en un rush.
            raw = body.get("rushes") or [body.get("rush")]
            parts = [safe_rush_name(r) for r in raw] if isinstance(raw, list) and 1 <= len(raw) <= 20 else [None]
            if not all(parts) or not all(os.path.isfile(os.path.join(RUSHES, n)) for n in parts):
                return self.send_json({"error": "Rush introuvable dans public/rushes."}, 400)
            name = joined_name(parts) if len(parts) > 1 else parts[0]
            if len(parts) > 1:
                need = sum(os.path.getsize(os.path.join(RUSHES, n)) for n in parts) + (2 << 30)
                if shutil.disk_usage(RUSHES).free < need:
                    return self.send_json({"error": no_space(need, RUSHES)}, 507)
            langue = body.get("langue") or "fr"
            if langue not in LANGUAGES:
                return self.send_json({"error": "Langue inconnue."}, 400)
            steps = PREPARE_STEPS
            if body.get("instructions"):
                if not instructions_info() and not style_info()["videos"] and not style_info()["sons"]:
                    return self.send_json({"error": "Aucune consigne ni aucun élément de style choisi."}, 400)
                if not claude_connected():
                    return self.send_json({"error": "Claude Code n'est pas connecté : ouvrez-le une fois (lanceur « Claude Code »)."}, 400)
                steps = PREPARE_WITH_INSTRUCTIONS
            rec = paused_task() if job["state"] != "running" else None
            if rec and rec.get("args") == parts and not body.get("abandon"):
                # Même rush que la préparation en pause : elle reprend là où elle en était.
                error = resume_job()
                return self.send_json({"error": error}, 409) if error else self.send_json({"ok": True, "resumed": True}, 202)
            if rec:
                if not body.get("abandon"):  # la page demande d'abord confirmation
                    return self.send_json({"error": busy_error(), "paused": True}, 409)
                abandon_task()
            if job["state"] != "running":  # langue de la transcription, lue par prepare.sh
                os.makedirs(WORK, exist_ok=True)
                open(os.path.join(WORK, "langue.txt"), "w", encoding="utf-8").write(langue + "\n")
            if not start_job("prepare", steps, name, parts, reuse=bool(kept_parts(parts))):
                return self.send_json({"error": busy_error()}, 409)
            return self.send_json({"ok": True}, 202)
        variant = str(body.get("variant") or "travail")
        if not SAFE_ID.match(variant):
            return self.send_json({"error": "variante invalide"}, 400)
        doc = os.path.join(DB, "variantes", f"{variant}.json")
        if not os.path.exists(doc):
            # Variante jamais modifiée sur la page (rush tout juste préparé) : sa sélection
            # actuelle (les suggestions) est envoyée avec la demande.
            if not isinstance(body.get("keep"), list):
                return self.send_json({"error": f"variante « {variant} » pas encore enregistrée"}, 400)
            os.makedirs(os.path.dirname(doc), exist_ok=True)
            json.dump({"name": str(body.get("name") or variant), "keep": body["keep"],
                       "rush": os.path.basename(read_text(os.path.join(WORK, "rush.txt")) or ""),
                       "updatedAt": datetime.datetime.now().isoformat()},
                      open(doc, "w"), ensure_ascii=False, indent=1)
        if not start_job("regen", REGEN_STEPS, variant):
            return self.send_json({"error": busy_error()}, 409)
        return self.send_json({"ok": True}, 202)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8080)
    args = parser.parse_args()
    chat_load()
    close_interrupted_logs()
    remove_partial_files(uploads=True)
    threading.Thread(target=update_loop, daemon=True).start()
    for m in chat["messages"]:  # réponse coupée par un arrêt du serveur
        if m.get("role") == "assistant" and not m.get("done"):
            m.update(done=True, parts=m.get("parts", []) + [{"kind": "error", "text": "Interrompu (serveur redémarré)."}])
    print(f"Auto-montage : http://localhost:{args.port}/ (moments) et /downloads/ (téléchargements)")
    ThreadingHTTPServer((args.host, args.port), Handler).serve_forever()
