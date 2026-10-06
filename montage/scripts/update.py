"""Mises à jour d'Auto-montage depuis les versions publiées sur GitHub (page « Mises à jour »).

Usage : python3 scripts/update.py check          dernière version publiée (JSON)
        python3 scripts/update.py apply <vX.Y>   télécharge cette version et remplace les
                                                 fichiers du programme
        python3 scripts/update.py reset [vX.Y]   réinitialisation : retélécharge la version
                                                 installée (par défaut) et remet les fichiers
                                                 du programme modifiés ou effacés comme publiés
Le code de la version (archive du tag) remplace celui du dossier d'installation ; ce qui
appartient à l'utilisateur reste : montage/work (montages, réglages), montage/out (rendus),
rushes, sons personnels, police Oliver, paquets Node. Les fichiers retirés d'une version à
l'autre sont effacés (liste .auto-montage-files). La réinitialisation garde une copie des
fichiers qu'elle remplace dans montage/work/sauvegardes/reinitialisation-<date>/.
"""
import datetime
import io
import json
import os
import re
import shutil
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(__file__))
from progress import report  # noqa: E402

MONTAGE = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
PROJECT = os.path.dirname(MONTAGE)  # dossier d'installation (/app dans le conteneur)
REPO = os.environ.get("AM_UPDATE_REPO", "RobinKD/Auto-montage")
BACKUPS = os.path.join(MONTAGE, "work", "sauvegardes")
MANIFEST = os.path.join(PROJECT, ".auto-montage-files")
# Jamais remplacés ni effacés : les données de l'utilisateur et ce qui est installé à part.
KEEP = ("montage/work/", "montage/out/", "montage/public/rushes/", "montage/public/sfx/perso/",
        "montage/node_modules/", "montage/whisper.cpp/", "montage/public/audio/",
        # Montage en cours (réécrits par build_edit.py) : pas remplacés par ceux du dépôt.
        "montage/src/data/edit.json", "montage/src/data/face.json", "montage/src/data/landmarks.json")
SKIP = ("packaging/", ".github/")  # pas dans les installations


def version_tuple(v):
    return tuple(int(x) for x in re.findall(r"\d+", v or "0"))


def current_version():
    path = os.path.join(PROJECT, "VERSION")
    return open(path).read().strip() if os.path.exists(path) else "0"


def api(url, accept="application/vnd.github+json"):
    req = urllib.request.Request(url, headers={"Accept": accept, "User-Agent": "auto-montage-updater",
                                               "X-GitHub-Api-Version": "2022-11-28"})
    return urllib.request.urlopen(req, timeout=60)


def check():
    """{"current", "latest", "tag", "newer", "notes", "error"}."""
    out = {"current": current_version(), "latest": None, "tag": None, "newer": False, "notes": "", "error": None}
    try:
        releases = json.load(api(f"https://api.github.com/repos/{REPO}/releases?per_page=20"))
    except urllib.error.HTTPError as e:
        out["error"] = ("GitHub limite les vérifications depuis cette connexion : réessayez dans une heure."
                        if e.code in (403, 429) else f"GitHub ne répond pas correctement (erreur {e.code}).")
        return out
    except (urllib.error.URLError, OSError, ValueError) as e:
        out["error"] = f"Pas de connexion à GitHub ({getattr(e, 'reason', e)})."
        return out
    tags = [r for r in releases if re.fullmatch(r"v\d+(\.\d+)*", r.get("tag_name", "")) and not r.get("draft")]
    if not tags:
        out["error"] = "Aucune version publiée trouvée."
        return out
    best = max(tags, key=lambda r: version_tuple(r["tag_name"]))
    # Nouveautés de toutes les versions plus récentes que celle installée (la plus récente en
    # premier), pour qui saute plusieurs versions.
    newer = sorted((r for r in tags if version_tuple(r["tag_name"]) > version_tuple(out["current"])),
                   key=lambda r: version_tuple(r["tag_name"]), reverse=True) or [best]
    notes = "\n\n".join(
        f"## Version {r['tag_name'][1:]}\n" + (r.get("body") or "").replace("\r\n", "\n").split("\n## Télécharger")[0].replace("## Nouveautés", "").strip()
        for r in newer[:15])
    out.update(latest=best["tag_name"][1:], tag=best["tag_name"], notes=notes[:20000],
               newer=version_tuple(best["tag_name"]) > version_tuple(out["current"]))
    return out


def protected(rel):
    return rel.startswith(KEEP) or rel.startswith("montage/public/fonts/Oliver")


def download(tag):
    """Archive .tar.gz de la version, lue en entier (barre « Téléchargement »)."""
    print(f"Téléchargement d'Auto-montage {tag[1:]}…", flush=True)
    local = os.environ.get("AM_UPDATE_ARCHIVE")  # essais : archive .tar.gz locale au lieu de GitHub
    resp = open(local, "rb") if local else api(f"https://api.github.com/repos/{REPO}/tarball/{tag}")
    total = int(resp.headers.get("Content-Length") or 0) if hasattr(resp, "headers") else os.path.getsize(local)
    buf = io.BytesIO()
    while True:
        chunk = resp.read(1 << 16)
        if not chunk:
            break
        buf.write(chunk)
        report("Téléchargement", buf.tell(), max(total, buf.tell(), 1))
    buf.seek(0)
    print(f"Archive reçue ({buf.getbuffer().nbytes >> 10} ko)…", flush=True)
    return buf


def unpack(buf, tmp):
    """Extrait l'archive dans tmp : (dossier source, fichiers du programme à installer)."""
    with tarfile.open(fileobj=buf, mode="r:gz") as tar:
        members = [m for m in tar.getmembers() if m.isfile() or m.isdir()]
        for m in members:  # archive GitHub : un dossier racine « owner-repo-sha/ »
            if m.name.startswith("/") or ".." in m.name.split("/"):
                sys.exit(f"Archive suspecte : {m.name}")
        tar.extractall(tmp, members=members)
    roots = os.listdir(tmp)
    if len(roots) != 1:
        sys.exit("Archive inattendue.")
    src = os.path.join(tmp, roots[0])
    files = []
    for d, _, names in os.walk(src):
        for n in names:
            rel = os.path.relpath(os.path.join(d, n), src).replace(os.sep, "/")
            if rel.startswith(SKIP) or protected(rel):
                continue
            files.append(rel)
    if "VERSION" not in files or not os.path.exists(os.path.join(src, "montage", "scripts", "local_server.py")):
        sys.exit("L'archive ne ressemble pas à Auto-montage : rien n'est remplacé.")
    return src, sorted(files)


def install(src, rel):
    dest = os.path.join(PROJECT, rel)
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    tmp_dest = dest + ".maj"
    shutil.copyfile(os.path.join(src, rel), tmp_dest)  # contenu seul, pas les droits de l'archive
    os.replace(tmp_dest, dest)
    if rel.endswith(".sh"):
        os.chmod(dest, 0o755)


def apply(tag):
    if not re.fullmatch(r"v\d+(\.\d+)*", tag):
        sys.exit(f"Version invalide : {tag}")
    buf = download(tag)
    print("Installation…", flush=True)
    with tempfile.TemporaryDirectory() as tmp:
        src, files = unpack(buf, tmp)
        old = set(open(MANIFEST).read().split("\n")) if os.path.exists(MANIFEST) else set()
        for i, rel in enumerate(files):
            install(src, rel)
            report("Installation", i + 1, len(files))
        removed = 0
        for rel in sorted(old - set(files)):  # retirés dans la nouvelle version
            path = os.path.realpath(os.path.join(PROJECT, rel))
            if not path.startswith(os.path.realpath(PROJECT) + os.sep):
                continue  # liste modifiée : rien hors de l'installation
            if rel and not protected(rel) and not rel.startswith(SKIP) and os.path.isfile(path):
                os.remove(path)
                removed += 1
        open(MANIFEST, "w").write("\n".join(files))
        open(os.path.join(PROJECT, ".auto-montage-version"), "w").write(tag[1:] + "\n")
    print(f"Auto-montage {tag[1:]} installé : {len(files)} fichiers remplacés, {removed} retirés.", flush=True)


def same(a, b):
    if os.path.getsize(a) != os.path.getsize(b):
        return False
    with open(a, "rb") as fa, open(b, "rb") as fb:
        return fa.read() == fb.read()


def reset(tag=None):
    """Remet les fichiers du programme comme dans la version publiée (celle installée par
    défaut) : fichiers modifiés ou effacés depuis l'installation, par exemple par Claude Code
    lancé dans le dossier. Les données de l'utilisateur (KEEP) ne sont ni lues ni touchées ;
    chaque fichier remplacé est d'abord copié dans work/sauvegardes/."""
    tag = tag or f"v{current_version()}"
    buf = None
    if re.fullmatch(r"v\d+(\.\d+)*", tag):
        try:
            buf = download(tag)
        except urllib.error.HTTPError as e:
            if e.code != 404:
                raise
    if buf is None:
        # Version installée pas publiée (fichiers d'une branche de travail, VERSION abîmé) :
        # la dernière version publiée.
        latest = check()
        if not latest.get("tag"):
            sys.exit(latest.get("error") or "Aucune version publiée trouvée.")
        print(f"La version {tag[1:]} n'est pas publiée sur GitHub : réinitialisation avec la "
              f"dernière version publiée, {latest['latest']}.", flush=True)
        tag = latest["tag"]
        buf = download(tag)
    print("Comparaison des fichiers…", flush=True)
    with tempfile.TemporaryDirectory() as tmp:
        src, files = unpack(buf, tmp)
        changed, missing = [], []
        for i, rel in enumerate(files):
            dest = os.path.join(PROJECT, rel)
            if not os.path.isfile(dest):
                missing.append(rel)
            elif not same(dest, os.path.join(src, rel)):
                changed.append(rel)
            report("Comparaison", i + 1, len(files))
        backup = None
        if changed:
            backup = os.path.join(BACKUPS, "reinitialisation-" + datetime.datetime.now().strftime("%Y-%m-%d_%H%M%S"))
            for rel in changed:
                os.makedirs(os.path.dirname(os.path.join(backup, rel)), exist_ok=True)
                shutil.copyfile(os.path.join(PROJECT, rel), os.path.join(backup, rel))
        todo = changed + missing
        for i, rel in enumerate(todo):
            install(src, rel)
            report("Réinitialisation", i + 1, len(todo))
        open(MANIFEST, "w").write("\n".join(files))
        open(os.path.join(PROJECT, ".auto-montage-version"), "w").write(tag[1:] + "\n")
    for rel in changed:
        print(f"  remis comme publié : {rel}", flush=True)
    for rel in missing:
        print(f"  recréé : {rel}", flush=True)
    if backup:
        print(f"Copie des fichiers remplacés : {os.path.relpath(backup, PROJECT)}", flush=True)
    n = lambda k, word: f"{k} {word}{'s' if k > 1 else ''}"  # noqa: E731
    print(f"Auto-montage {tag[1:]} réinitialisé : {n(len(changed), 'fichier')} remis comme publiés, "
          f"{n(len(missing), 'recréé')}, {n(len(files) - len(todo), 'déjà identique')}.", flush=True)


if __name__ == "__main__":
    if len(sys.argv) >= 2 and sys.argv[1] == "check":
        print(json.dumps(check(), ensure_ascii=False))
    elif len(sys.argv) == 3 and sys.argv[1] == "apply":
        apply(sys.argv[2])
    elif len(sys.argv) in (2, 3) and sys.argv[1] == "reset":
        reset(sys.argv[2] if len(sys.argv) == 3 else None)
    else:
        sys.exit(__doc__)
