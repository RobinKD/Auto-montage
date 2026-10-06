"""Rendu du montage avec HyperFrames : le montage (src/data/edit.json, face.json) est écrit en
page HTML + GSAP dans work/rendu_hyperframes/ puis rendu par « hyperframes render »
(Chrome sans écran, ffmpeg). Effets dessinés image par image en JavaScript (zooms, secousses,
flashs, textes tapés, pastilles, sous-titres avec mots mis en valeur, mot prononcé en couleur et
rebond) ; la coupure des sous-titres en 2 lignes est calculée ici (render_lib).

Usage : python3 scripts/render_hyperframes.py apercu|final <sortie.mp4>
  final : source 4K (make_hd.py), page rendue à l'échelle 2 (2160x3840, 3840x2160 en paysage,
  2880x2160 en 4:3…).
"""
import json
import os
import re
import shutil
import subprocess
import sys
import wave

import imageio_ffmpeg

sys.path.insert(0, os.path.dirname(__file__))
from progress import report  # noqa: E402
from moments_lib import HF_VOICES, frame_layout  # noqa: E402
from render_lib import POPPINS, ROOT, Montage, headless_chrome  # noqa: E402

DIR = os.path.join(ROOT, "work", "rendu_hyperframes")
HF_AUDIO = os.path.join(ROOT, "selection", "hf_audio.js")
NODE_MODULES = os.path.join(ROOT, "node_modules")
# 4K : préréglages de « hyperframes render --resolution » (même page, rendue à l'échelle 2) ; les
# autres formats (4:3…) sont agrandis dans la page elle-même (build, k = 2).
HD_PRESETS = {(1080, 1920): "portrait-4k", (1920, 1080): "landscape-4k", (1080, 1080): "square-4k"}


def wav_len(path):
    try:
        with wave.open(path) as w:
            return w.getnframes() / w.getframerate()
    except (OSError, wave.Error, EOFError):
        return 1e9


def link(src, name):
    """Fichier du projet HyperFrames (lien vers l'original, sans copie quand c'est possible)."""
    dst = os.path.join(DIR, "assets", name)
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    if os.path.lexists(dst):
        os.remove(dst)
    try:
        os.link(src, dst)
    except OSError:
        try:
            os.symlink(src, dst)
        except OSError:
            shutil.copy(src, dst)
    return "assets/" + name


def voice_audio(src, duration, e):
    """Éléments audio de la voix, avec les voix modifiées de HF_VOICES, les graves, la clarté et
    le limiteur faits par HyperFrames : selection/hf_audio.js (le même code que l'aperçu de la
    page des moments), lancé avec node. Pistes à partir de 1 ; les bruitages sont sur les pistes 20 et plus."""
    spec = {"src": src, "start": 0, "mediaStart": 0, "duration": duration, "volume": 1, "track": 1,
            "son": e.get("son") or {},
            "windows": [{"a": f["start"], "b": f["end"], "voice": f["voice"], "force": f.get("force", 1)}
                        for f in e.get("funny") or [] if f.get("voice") in HF_VOICES]}
    js = ("const H = require(process.argv[1]); let s = ''; process.stdin.on('data', (d) => s += d);"
          "process.stdin.on('end', () => process.stdout.write(H.voiceElements(JSON.parse(s))));")
    return subprocess.run(["node", "-e", js, HF_AUDIO], input=json.dumps(spec), capture_output=True, text=True,
                          check=True).stdout


def build(m, k=1):
    """Page du montage ; k : agrandissement fait dans la page (2 pour une 4K sans préréglage
    « --resolution » de HyperFrames : 4:3 et autres formats que 9:16, 16:9 et carré)."""
    e, fps, n = m.edit, m.fps, m.frames
    r6 = lambda x: f"{x:.6f}".rstrip("0").rstrip(".")
    rush, key = m.rush
    src = link(rush, os.path.basename(rush))
    # Plans : un <video> par plan (chaque élément a un id : sans lui, HyperFrames le fige ou le
    # coupe), début arrondi à l'image inférieure, lecture un quart d'image après le début du plan
    # (l'image exacte du plan, sans glisser sur la précédente).
    videos = "\n".join(
        f'<video id="clip{i}" class="clip" muted playsinline src="{src}" data-start="{int(c["from"] / fps * 1e6) / 1e6}" '
        f'data-duration="{r6(c["durationInFrames"] / fps)}" data-media-start="{r6((c[key] + 0.25) / fps)}" '
        f'data-track-index="0"></video>' for i, c in enumerate(e["clips"]))
    voice = os.path.join(ROOT, "public", "audio", "voice.wav")
    audios = [voice_audio(link(voice, "voice.wav"), min(n / fps, wav_len(voice)), e)]
    for i, (path, start, dur, vol) in enumerate(m.sfx()):
        dur = min(dur, wav_len(path), n / fps - start)
        if dur > 0:
            name = "sfx/" + os.path.relpath(path, os.path.join(ROOT, "public")).replace("/", "_")
            audios.append(f'<audio id="sfx{i}" src="{link(path, name)}" data-start="{r6(start)}" data-duration="{r6(dur)}" '
                          f'data-track-index="{20 + i % 8}" data-volume="{vol}"></audio>')
    # Sous-titres : mots, couleurs des mots mis en valeur, lignes (et leur taille) et réduction.
    caps = []
    for c in e.get("captions") or []:
        words = m.caption_words(c)
        colors = m.caption_colors(c, words)
        lines, scale = m.caption_layout(words, [x is not None for x in colors])
        caps.append({"start": c["start"], "end": c["end"], "words": words, "colors": colors, "lines": lines,
                     "scale": scale, "times": c.get("times") or []})
    cs = m.cap
    font_ext = os.path.splitext(m.font_file)[1]
    data = {"fps": fps, "frames": n, "clips": [{k: c[k] for k in ("from", "durationInFrames", "trimBefore")} for c in e["clips"]],
            "face": m.face, "zooms": e.get("zooms") or [], "zoomFx": e.get("zoomFx") or [], "shakes": e.get("shakes") or [],
            "flashes": e.get("flashes") or [], "typed": m.typed, "chips": e.get("chips") or [], "captions": caps,
            "hab": m.hab, "cap": {k: cs.get(k) for k in ("weight", "size", "uppercase", "color", "shadow", "blur", "ox", "oy")}}
    page = PAGE
    # Format du rush : sur une image basse (paysage, 4:3), sous-titres plus bas et texte tapé au centre.
    pos = frame_layout(m.width, m.height)
    layout = {"W": str(m.width), "H": str(m.height), "OUTW": str(k * m.width), "OUTH": str(k * m.height), "K": str(k),
              "CAPTOP": str(pos["capTop"]), "TYPEDTOP": str(pos["typedTop"])}
    for k, v in {**layout, "FPS": str(fps), "DUR": r6(n / fps), "VIDEOS": videos, "AUDIOS": "\n".join(audios),
                 "POPPINS": link(POPPINS, "fonts/poppins.ttf"), "CAPFONT": link(m.font_file, "fonts/caption" + font_ext),
                 "CAPWEIGHT": str(m.font_weight), "DATA": json.dumps(data, ensure_ascii=False).replace("<", "\\u003c")}.items():
        page = page.replace("%" + k + "%", v)
    open(os.path.join(DIR, "index.html"), "w", encoding="utf-8").write(page)
    shutil.copy(os.path.join(NODE_MODULES, "gsap", "dist", "gsap.min.js"), os.path.join(DIR, "assets", "gsap.min.js"))


def tools():
    """ffmpeg (imageio-ffmpeg), ffprobe (@ffprobe-installer) et Chrome sans écran (render_lib)."""
    env = dict(os.environ, HYPERFRAMES_NO_TELEMETRY="1", DO_NOT_TRACK="1", HYPERFRAMES_NO_UPDATE_CHECK="1",
               HYPERFRAMES_NO_AUTO_INSTALL="1", HYPERFRAMES_FFMPEG_PATH=imageio_ffmpeg.get_ffmpeg_exe())
    probe = subprocess.run(["node", "-p", "require('@ffprobe-installer/ffprobe').path"], cwd=ROOT,
                           capture_output=True, text=True).stdout.strip()
    if not probe or not os.path.exists(probe):
        sys.exit("ffprobe introuvable (paquet npm @ffprobe-installer/ffprobe) : relancer « npm install ».")
    os.chmod(probe, 0o755)
    env["HYPERFRAMES_FFPROBE_PATH"] = probe

    shell = headless_chrome()
    if shell:
        env["PRODUCER_HEADLESS_SHELL_PATH"] = shell
    return env


def main():
    mode, out = sys.argv[1], sys.argv[2]
    hd = mode == "final"
    m = Montage(hd=hd)
    shutil.rmtree(DIR, ignore_errors=True)
    os.makedirs(os.path.join(DIR, "assets"))
    json.dump({"paths": {"assets": "assets"}}, open(os.path.join(DIR, "hyperframes.json"), "w"))
    preset = HD_PRESETS.get((m.width, m.height)) if hd else None
    build(m, 2 if hd and not preset else 1)
    # 4K : 2 navigateurs au plus (4 dépassent 14 Go de mémoire et le rendu est arrêté).
    workers = max(1, min(2 if hd else 4, os.cpu_count() or 1))
    cmd = ["node", os.path.join(NODE_MODULES, "hyperframes", "dist", "cli.js"), "render", DIR, "-o", os.path.abspath(out),
           "--workers", str(workers), "--crf", "16" if hd else "20"]
    if preset:
        cmd += ["--resolution", preset]
    proc = subprocess.Popen(cmd, cwd=ROOT, env=tools(), stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    buf = b""
    report("Rendu de la vidéo", 0, 100)

    def line(raw):
        text = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", raw.decode("utf-8", "replace")).strip()
        pct = re.search(r"(\d{1,3})%\s+(\S.*)$", text)
        if pct:
            report("Rendu de la vidéo", min(100, int(pct.group(1))), 100)
        elif text and not text.startswith(("[INFO]", "|", "o ")) and not set(text) <= set("█░ "):
            print(text, flush=True)
    for chunk in iter(lambda: proc.stdout.read1(4096), b""):
        buf += chunk
        *lines, buf = re.split(rb"[\r\n]", buf)
        for raw in lines:
            line(raw)
    line(buf)
    if proc.wait() or not os.path.exists(out):
        sys.exit(f"Échec du rendu HyperFrames (code {proc.returncode})")
    report("Rendu de la vidéo", 100, 100)
    shutil.rmtree(os.path.join(DIR, "assets"), ignore_errors=True)  # liens vers le rush : inutiles ensuite
    print(f"Rendu HyperFrames : {out}")


PAGE = r"""<!doctype html>
<html lang="fr">
<head>
<meta charset="UTF-8" />
<meta name="viewport" content="width=%OUTW%, height=%OUTH%" />
<title>Montage</title>
<script src="assets/gsap.min.js"></script>
<style>
@font-face { font-family: "AM Poppins"; src: url("%POPPINS%"); font-weight: 900; }
@font-face { font-family: "AM Sous-titres"; src: url("%CAPFONT%"); font-weight: %CAPWEIGHT%; }
html, body { margin: 0; background: black; }
#root { position: relative; width: %OUTW%px; height: %OUTH%px; overflow: hidden; background: black; }
#frame { position: absolute; left: 0; top: 0; width: %W%px; height: %H%px; overflow: hidden; transform: scale(%K%); transform-origin: 0 0; }
.fill { position: absolute; inset: 0; display: flex; flex-direction: column; }
.clip { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
#typed { justify-content: center; align-items: center; padding-top: %TYPEDTOP%px; box-sizing: border-box; }
#typed div { font-family: "AM Poppins"; font-weight: 900; font-size: 92px; line-height: 1.02; color: white;
  text-align: center; white-space: pre-line; text-shadow: 0 3px 12px rgba(0,0,0,0.35), 0 1px 3px rgba(0,0,0,0.3); padding: 0 70px; }
#chips { align-items: center; padding-top: 170px; box-sizing: border-box; }
#chips div { font-family: "AM Poppins"; font-weight: 900; font-size: 50px; line-height: 1; color: #1d1d1b; background: white;
  padding: 22px 36px; border-radius: 999px; box-shadow: 0 8px 24px rgba(0,0,0,0.25); }
#caps { align-items: center; top: %CAPTOP%px; }
#caps > div { display: flex; flex-direction: column; align-items: center; gap: 4px; }
#flash { background: white; }
</style>
</head>
<body>
<div id="root" data-composition-id="main" data-start="0" data-duration="%DUR%" data-fps="%FPS%" data-width="%OUTW%" data-height="%OUTH%">
<div id="frame">
<div id="zoom" class="fill">
%VIDEOS%
</div>
%AUDIOS%
<div id="typed" class="fill"></div>
<div id="chips" class="fill"></div>
<div id="caps" class="fill"></div>
<div id="flash" class="fill" style="opacity:0"></div>
</div>
</div>
<script>
// Chaque image est dessinée d'après son numéro (timeline GSAP en pause,
// déplacée image par image par HyperFrames).
const D = %DATA%;
const FPS = D.fps, HL = 1.2;
const inRange = (t, r) => t >= r.start && t < r.end;
const clamp = (x, a, b) => Math.min(b, Math.max(a, x));
const ease = (x) => { x = clamp(x, 0, 1); return x < 0.5 ? 2 * x * x : 1 - Math.pow(-2 * x + 2, 2) / 2; };
const spring = (frame, damping, mass, from = 0, to = 1) => {
  if (frame < 0) return from;
  const t = frame / FPS, k = 100, zeta = damping / (2 * Math.sqrt(k * mass)), w0 = Math.sqrt(k / mass);
  let x;
  if (zeta < 1) { const w1 = w0 * Math.sqrt(1 - zeta * zeta); x = Math.exp(-zeta * w0 * t) * (zeta * w0 / w1 * Math.sin(w1 * t) + Math.cos(w1 * t)); }
  else x = Math.exp(-w0 * t) * (1 + w0 * t);
  return from + (to - from) * (1 - x);
};
const sourceTime = (f) => {
  const c = D.clips.find((c) => f >= c.from && f < c.from + c.durationInFrames) || D.clips[D.clips.length - 1];
  return (c.trimBefore + f - c.from) / FPS;
};
const faceAt = (t) => {
  const F = D.face, i = Math.min(F.length - 2, Math.max(0, Math.floor(t / 0.5))), a = F[i], b = F[i + 1];
  const k = clamp((t - a.t) / ((b.t - a.t) || 1), 0, 1);
  return { x: a.x + (b.x - a.x) * k, y: a.y + (b.y - a.y) * k };
};
const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
// Ombre : intensité (0-100, plus ou moins sombre), diffusion du flou et décalage (ox, oy), en px sur 1080.
// Même formule dans la page des moments (captionShadow de selection/page.html).
const captionShadow = (shadow = 50, blur = 14, ox = 0, oy = 0) => {
  if (shadow <= 0) return "none";
  const s = shadow / 100, at = (b, a) => `${ox}px ${oy}px ${b}px rgba(0,0,0,${Math.min(1, a).toFixed(3)})`;
  const layers = [at(blur, 1.2 * s), at(blur / 2, 0.8 * s)];
  if (s > 0.5) layers.push(at(blur / 4, 1.6 * (s - 0.5)));
  return layers.join(", ");
};
const C = D.cap;
const capStyle = `font-family:"AM Sous-titres";font-weight:${C.weight};color:${C.color};text-transform:${C.uppercase ? "uppercase" : "none"};` +
  `text-align:center;line-height:1.05;white-space:nowrap;text-shadow:${captionShadow(C.shadow ?? 50, C.blur ?? 14, C.ox ?? 0, C.oy ?? 0)};`;
const el = (id) => document.getElementById(id);
const last = {};
const put = (id, html) => { if (last[id] !== html) { last[id] = html; el(id).innerHTML = html; } };

function draw(frame) {
  const t = frame / FPS, H = D.hab;
  // Zooms automatiques, zoom placé sur un moment, secousse.
  const zoom = D.zooms.find((z) => inRange(t, z)) || D.zooms[D.zooms.length - 1] || { type: "in", start: 0, end: 1 };
  let scale = zoom.type === "in" ? 1 + 0.12 * H.autoZoom * ease((t - zoom.start) / Math.max(0.01, zoom.end - zoom.start)) : 1 + 0.22 * H.autoZoom;
  const m = D.zoomFx.find((z) => inRange(t, z));
  if (m) { const p = ease((t - m.start) / Math.max(0.01, m.anim)); scale = m.type === "in" ? 1 + m.force * p : m.type === "punch" ? 1 + m.force : 1 + m.force * (1 - p); }
  const sh = D.shakes.find((x) => inRange(t, x));
  let dx = 0, dy = 0;
  if (sh) {
    const damp = 1 - (t - sh.start) / Math.max(0.01, sh.end - sh.start);
    dx = 40 * sh.force * damp * (Math.sin(t * 71) + 0.5 * Math.sin(t * 37));
    dy = 40 * sh.force * damp * (Math.cos(t * 59) + 0.5 * Math.sin(t * 43));
  }
  const f = faceAt(sourceTime(frame));
  const z = el("zoom");
  z.style.transform = `translate(${dx}px, ${dy}px) scale(${scale})`;
  z.style.transformOrigin = `${f.x * 100}% ${f.y * 100}%`;

  // Texte tapé.
  const ty = D.typed.find((x) => inRange(t, x));
  if (ty) {
    const chars = ty.text.replace(/\n/g, "").length;
    const shown = Math.floor(clamp((t - ty.start) / Math.max(1e-6, ty.typeDuration), 0, 1) * chars);
    let count = 0, visible = "";
    for (const ch of ty.text) { if (ch === "\n") { visible += ch; continue; } if (count >= shown) break; visible += ch; count++; }
    const cursor = Math.floor(t * 3) % 2 === 0 || shown < chars;
    el("typed").style.opacity = clamp((ty.end - t) / 0.15, 0, 1);
    put("typed", `<div>${esc(visible)}<span style="opacity:${cursor ? 1 : 0}">|</span></div>`);
  } else put("typed", "");

  // Pastille.
  const chip = D.chips.find((c) => inRange(t, c));
  if (chip) {
    const s = spring(frame - Math.round(chip.start * FPS), 11, 0.6) * clamp((chip.end - t) / 0.15, 0, 1);
    put("chips", `<div style="transform:scale(${s})">${esc(chip.text)}</div>`);
  } else put("chips", "");

  // Sous-titres (pas pendant un texte tapé).
  const cap = ty ? null : D.captions.find((c) => inRange(t, c));
  if (cap) {
    let cur = -1;
    if (H.karaoke) cap.times.forEach((x, i) => { if (t >= x) cur = i; });
    const pop = H.pop ? spring(frame - Math.round(cap.start * FPS), 10, 0.5, 0.75, 1) : 1;
    const html = cap.lines.map(([a, b, k]) => `<div style='${capStyle}font-size:${C.size * k * cap.scale}px'>` + cap.words.slice(a, b).map((w, j) => {
      const i = a + j, c = i === cur ? H.karaokeColor : cap.colors[i];
      return `<span style="${c ? `color:${c};` : ""}${cap.colors[i] ? `font-size:${HL}em;` : ""}">${j ? " " : ""}${esc(w)}</span>`;
    }).join("") + "</div>").join("");
    put("caps", `<div style="transform:scale(${pop})">${html}</div>`);
    // Lignes nombreuses ou grandes : sous-titre remonté pour finir à 40 px au moins du bas de l'image.
    const over = el("caps").offsetTop + el("caps").firstElementChild.offsetHeight - (%H% - 40);
    el("caps").style.translate = over > 0 ? `0 ${-over}px` : "";
  } else { put("caps", ""); el("caps").style.translate = ""; }

  // Flash blanc.
  const fl = D.flashes.find((x) => inRange(t, x));
  const p = fl ? (t - fl.start) / Math.max(0.01, fl.end - fl.start) : 0;
  el("flash").style.opacity = fl ? 0.95 * (1 - p) * Math.min(1, p * 8 + 0.4) : 0;
}

// Le numéro d'image suit le temps de la timeline (propriété d'un objet animée par GSAP).
let shownFrame = -1;
const clock = {
  get t() { return shownFrame / FPS; },
  set t(v) { const f = Math.min(D.frames - 1, Math.max(0, Math.round(v * FPS))); if (f !== shownFrame) { shownFrame = f; draw(f); } },
};
const tl = gsap.timeline({ paused: true });
tl.fromTo(clock, { t: 0 }, { t: %DUR%, duration: %DUR%, ease: "none", immediateRender: true }, 0);
clock.t = 0;
window.__timelines = window.__timelines || {};
window.__timelines["main"] = tl;
</script>
</body>
</html>
"""

if __name__ == "__main__":
    main()
