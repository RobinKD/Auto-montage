"""Plusieurs vidéos pour un même rush (caméra coupée en cours de tournage, batterie, carte
pleine…) : assemblées dans l'ordre en une seule vidéo, que la préparation traite ensuite
comme n'importe quel rush.

Usage : python3 scripts/join_rushes.py <vidéo 1> <vidéo 2> [<vidéo 3>…]   assemble
        python3 scripts/join_rushes.py --name <vidéo 1> <vidéo 2> […]     nom du rush assemblé
Les vidéos sont dans public/rushes/ ; le rush assemblé aussi (« <vidéo 1> (assemblage de N).mov »).
Vidéos de mêmes réglages (codec, taille, cadence, son) : mises bout à bout sans réencodage
(quelques secondes, qualité d'origine). Réglages différents : réencodées aux réglages de la
première (taille, cadence ; H.264 haute qualité), plus long. Déjà assemblées à l'identique
(mêmes vidéos, même ordre) : rien n'est refait.
"""
import json
import os
import re
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(__file__))
import progress  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
RUSHES = os.path.join(ROOT, "public", "rushes")


def joined_name(names):
    stem = os.path.splitext(os.path.basename(names[0]))[0][:150]
    return f"{stem} (assemblage de {len(names)}).mov"


def ffmpeg_exe():
    import imageio_ffmpeg
    return imageio_ffmpeg.get_ffmpeg_exe()


def flat(text):
    """Retire les parenthèses (« yuv420p(tv, bt709) ») : restent les champs séparés par des virgules."""
    while True:
        new = re.sub(r"\([^()]*\)", "", text)
        if new == text:
            return [f.strip() for f in text.split(",")]
        text = new


def probe(path):
    """Réglages d'une vidéo, lus dans la sortie de « ffmpeg -i »."""
    out = subprocess.run([ffmpeg_exe(), "-hide_banner", "-i", path], capture_output=True, text=True).stderr
    m = re.search(r"Duration: (\d+):(\d+):([\d.]+)", out)
    info = {"duration": int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3]) if m else 0.0}
    video = re.search(r"Stream #\d+:\d+.*?: Video: (.*)", out)
    if not video:
        sys.exit(f"Pas d'image dans {os.path.basename(path)} : est-ce bien une vidéo ?")
    fields = flat(video[1])
    size = next((re.match(r"(\d+)x(\d+)", f) for f in fields if re.match(r"\d+x\d+", f)), None)
    fps = re.search(r"([\d.]+) fps", video[1]) or re.search(r"([\d.]+) tbr", video[1])
    rot = re.search(r"rotation of (-?[\d.]+) degrees", out)
    info.update(vcodec=fields[0].split()[0], pix=fields[1] if len(fields) > 1 else "",
                width=int(size[1]) if size else 0, height=int(size[2]) if size else 0,
                fps=float(fps[1]) if fps else 30.0, rotation=round(float(rot[1])) % 360 if rot else 0)
    audio = re.search(r"Stream #\d+:\d+.*?: Audio: (.*)", out)
    if audio:
        a = flat(audio[1])
        info.update(acodec=a[0].split()[0], rate=a[1] if len(a) > 1 else "", layout=a[2] if len(a) > 2 else "")
    else:
        info.update(acodec=None, rate=None, layout=None)
    return info


def same_settings(infos):
    keys = ("vcodec", "pix", "width", "height", "fps", "rotation", "acodec", "rate", "layout")
    return all(info["acodec"] for info in infos) and all(
        tuple(i[k] for k in keys) == tuple(infos[0][k] for k in keys) for i in infos[1:])


def signature(paths):
    return [[os.path.basename(p), os.path.getsize(p), int(os.path.getmtime(p))] for p in paths]


def join(names):
    paths = [os.path.join(RUSHES, os.path.basename(n)) for n in names]
    for p in paths:
        if not os.path.isfile(p):
            sys.exit(f"Vidéo introuvable : {p}")
    out = os.path.join(RUSHES, joined_name(names))
    meta = os.path.join(RUSHES, f".{os.path.basename(out)}.sources.json")
    try:
        if os.path.isfile(out) and json.load(open(meta)) == signature(paths):
            print(f"Déjà assemblé : {os.path.basename(out)}")
            return out
    except (OSError, ValueError):
        pass
    infos = [probe(p) for p in paths]
    total = sum(i["duration"] for i in infos)
    part = out[:-4] + ".part.mov"
    ffmpeg = ffmpeg_exe()
    if same_settings(infos):
        print(f"Assemblage de {len(paths)} vidéos aux mêmes réglages, sans réencodage", flush=True)
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            for p in paths:
                if "\n" in p or "\r" in p:  # une ligne en plus ajouterait un autre fichier à la liste
                    sys.exit(f"Nom de fichier invalide (retour à la ligne) : {p!r}")
                f.write("file '" + p.replace("'", "'\\''") + "'\n")
            listing = f.name
        try:
            code = progress.ffmpeg("Assemblage des vidéos", [
                ffmpeg, "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", listing,
                "-map", "0:v:0", "-map", "0:a:0", "-c", "copy", "-movflags", "+faststart", part], total=total)
        finally:
            os.remove(listing)
    else:
        first = infos[0]
        # Taille affichée de la première (vidéo de téléphone tournée : largeur et hauteur inversées).
        w, h = (first["height"], first["width"]) if first["rotation"] in (90, 270) else (first["width"], first["height"])
        w, h = w - w % 2, h - h % 2
        fps = first["fps"] if 1 <= first["fps"] <= 120 else 30
        print(f"Assemblage de {len(paths)} vidéos aux réglages différents : réencodage en {w}x{h}, "
              f"{fps:g} images/s (plus long)", flush=True)
        for p, i in zip(paths, infos):
            print(f"  {os.path.basename(p)} : {i['width']}x{i['height']}, {i['fps']:g} images/s, {i['vcodec']}, "
                  f"son {i['acodec'] or 'absent'}", flush=True)
        cmd = [ffmpeg, "-v", "error", "-y"]
        for p in paths:
            cmd += ["-i", p]
        graph, n = [], len(paths)
        for k, i in enumerate(infos):
            if not i["acodec"]:  # vidéo sans son : silence de même durée
                cmd += ["-f", "lavfi", "-t", f"{i['duration']:.3f}", "-i", "anullsrc=r=48000:cl=stereo"]
        silent = n
        for k, i in enumerate(infos):
            graph.append(f"[{k}:v:0]scale={w}:{h}:force_original_aspect_ratio=decrease,"
                         f"pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps:g},format=yuv420p[v{k}]")
            src = f"{k}:a:0" if i["acodec"] else f"{silent}:a:0"
            if not i["acodec"]:
                silent += 1
            graph.append(f"[{src}]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{k}]")
        graph.append("".join(f"[v{k}][a{k}]" for k in range(n)) + f"concat=n={n}:v=1:a=1[v][a]")
        cmd += ["-filter_complex", ";".join(graph), "-map", "[v]", "-map", "[a]",
                "-c:v", "libx264", "-preset", "veryfast", "-crf", "16", "-pix_fmt", "yuv420p",
                "-c:a", "aac", "-b:a", "256k", "-movflags", "+faststart", part]
        code = progress.ffmpeg("Assemblage des vidéos (réencodage)", cmd, total=total)
    if code != 0:
        if os.path.exists(part):
            os.remove(part)
        sys.exit("L'assemblage des vidéos a échoué (voir les messages de ffmpeg ci-dessus).")
    os.replace(part, out)
    json.dump(signature(paths), open(meta, "w"))
    print(f"Rush assemblé : {os.path.basename(out)} ({total:.0f} s)")
    return out


if __name__ == "__main__":
    args = sys.argv[1:]
    if args[:1] == ["--name"] and len(args) >= 3:
        print(joined_name(args[1:]))
    elif len(args) >= 2 and not args[0].startswith("-"):
        join(args)
    else:
        sys.exit(__doc__)
