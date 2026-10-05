// Voix modifiées et nettoyage de la voix faits par HyperFrames (effets audio data-fx-chain),
// partagés par le rendu (scripts/render_hyperframes.py, lancé avec node) et l'aperçu de la
// page des moments (lecteur HyperFrames) : les deux construisent les mêmes éléments audio,
// donc l'aperçu sonne comme la version de travail.
// Les autres voix (moments_lib.FFMPEG_VOICES : tremblement, robot, radio, batterie) restent
// faites par ffmpeg dans build_edit.py : HyperFrames n'a ni modulation en anneau, ni bruit,
// ni ralentissement progressif.
(function (root) {
  "use strict";
  const clamp = (x, a, b) => Math.min(b, Math.max(a, x));
  const st = (ratio) => clamp(12 * Math.log2(ratio), -12, 12);  // demi-tons d'un rapport de hauteur
  const db = (x) => 20 * Math.log10(x);

  // Effets de chaque voix, force f de 0 à 2 (1 = réglage d'origine), d'après les filtres ffmpeg
  // d'origine (moments_lib.voice_filter), au niveau de la voix normale (± 2 dB).
  const VOICES = {
    megaphone: (f) => [
      ["highpass", { frequency: 600 }], ["lowpass", { frequency: 3200 }],
      ["gain", { gain: db(2 + 4 * f) }], ["saturate", { type: "atan", threshold: 0, output: -6 }],
      ["delay", { time: 12, feedback: 0.01, mix: 0.2 }],
    ],
    telephone: (f) => [
      ["highpass", { frequency: 400 }], ["lowpass", { frequency: 3000 }],
      ["bitcrush", { bits: 9, mix: Math.min(1, 0.35 * f) }], ["gain", { gain: 1.5 }],
    ],
    salle: (f) => [
      ["delay", { time: 90, feedback: Math.min(0.8, 0.55 * f), mix: Math.min(0.6, 0.3 * f) }],
      ["lowpass", { frequency: 7000 }], ["gain", { gain: 2 }],
    ],
    ecureuil: (f) => [["pitchshift", { semitones: st(1 + 0.6 * f), mix: 1 }]],
    lutin: (f) => [
      ["pitchshift", { semitones: st(1 + 0.3 * f), mix: 1 }],
      ["chorus", { delay: 30, depth: 2, speed: 1.5, mix: 0.35 }], ["gain", { gain: 3 }],
    ],
    geant: (f) => [
      ["pitchshift", { semitones: st(1 / (1 + 0.47 * f)), mix: 1 }],
      ["delay", { time: 60, feedback: 0.01, mix: 0.2 }], ["gain", { gain: 2 }],
    ],
    choeur: (f) => [
      ["chorus", { delay: 18, depth: Math.min(10, 3 * f), speed: 1.3, mix: 0.45 }],
      ["chorus", { delay: 31, depth: Math.min(10, 4 * f), speed: 0.8, mix: 0.4 }], ["gain", { gain: 7 }],
    ],
  };
  // Décalage de hauteur : ~50 ms de retard (moitié d'un grain de 100 ms), et 100 ms de son
  // non décalé en tête d'élément le temps de remplir le grain. L'élément commence donc plus tôt
  // (PRE, en silence) et lit le rush 50 ms en avance, pour rester calé sur l'image.
  const PITCH_LATENCY = 0.05, PRE = 0.15;
  const XF = 0.02;  // fondus enchaînés entre la voix normale et la voix modifiée

  const hasVoice = (voice) => Object.prototype.hasOwnProperty.call(VOICES, voice);
  const force = (f) => clamp(Number.isFinite(Number(f)) && f !== null ? Number(f) : 1, 0, 2);

  // Nettoyage de toute la voix (panneau « Son de la voix », work/son.json) : graves parasites
  // et clarté ; le bruit de fond et les clics restent faits par ffmpeg (build_edit.py).
  function cleanNodes(son) {
    const out = [];
    if (son && son.highpass) out.push(["highpass", { frequency: 80 }]);
    if (son && son.clarity) {
      out.push(["peaking", { frequency: 3000, gain: 3, q: 1 }]);
      out.push(["compressor", { threshold: -22, ratio: 3, attack: 5, release: 80, makeup: 2 }]);
    }
    return out;
  }
  // Limiteur à -1,5 dB : le rush touche déjà 0 dB, de la place pour les bruitages.
  const LIMITER = ["limiter", { limit: -1.5, attack: 5, release: 50 }];

  function chain(nodes) {
    return { version: 1, nodes: nodes.map(([type, params], i) => ({ type, id: `n${i + 1}`, params })) };
  }
  const voiceChain = (voice, f, son, limit = true) =>
    chain([...cleanNodes(son), ...(hasVoice(voice) ? VOICES[voice](force(f)) : []), ...(limit ? [LIMITER] : [])]);

  const num = (x) => String(Math.round(x * 1e6) / 1e6);
  const attr = (v) => JSON.stringify(v).replace(/&/g, "&amp;").replace(/"/g, "&quot;");

  // Éléments audio de la voix : la voix entière (« dry ») baissée à 0 pendant les fenêtres, et
  // un élément par fenêtre de voix modifiée, sur une autre piste, avec sa chaîne d'effets.
  //   src : fichier son ; start : départ dans la composition ; mediaStart : position dans le
  //   fichier au départ ; duration ; volume ; son : nettoyage ; limit : limiteur à la fin ;
  //   windows : [{a, b, voice, force}] en temps de la composition (voix HyperFrames) ;
  //   silent : [{a, b}] fenêtres où la voix entière se tait aussi (voix faites ailleurs) ;
  //   track : piste de la voix entière, les fenêtres sur les suivantes ; id : préfixe des
  //   identifiants.
  function voiceElements(o) {
    const start = o.start || 0, mediaStart = o.mediaStart || 0, end = start + o.duration;
    const vol = num(clamp(o.volume === undefined ? 1 : o.volume, 0, 1));
    const track = o.track || 1, id = o.id || "voix";
    const wins = (o.windows || []).filter((w) => hasVoice(w.voice) && w.b > w.a)
      .map((w) => ({ ...w, a: clamp(w.a, start, end), b: clamp(w.b, start, end) })).filter((w) => w.b - w.a > 0.02);
    const holes = [...wins, ...(o.silent || []).map((w) => ({ a: clamp(w.a, start, end), b: clamp(w.b, start, end) }))]
      .filter((w) => w.b > w.a).sort((x, y) => x.a - y.a)
      .reduce((acc, w) => {  // fenêtres qui se touchent : un seul creux
        const last = acc[acc.length - 1];
        if (last && w.a <= last.b + 2 * XF) last.b = Math.max(last.b, w.b); else acc.push({ a: w.a, b: w.b });
        return acc;
      }, []);
    const points = [{ t: 0, v: 1 }];
    for (const w of holes) {
      const a = w.a - start, b = w.b - start;
      points.push({ t: Math.max(0, a - XF), v: 1 }, { t: a, v: 0 }, { t: b, v: 0 }, { t: b + XF, v: 1 });
    }
    const lane = (pts) => attr({ version: 1, lanes: [{ target: "volume", points: pts.slice(0, 512) }] });
    const el = (eid, s, d, ms, tr, fx, auto) =>
      `<audio id="${eid}" src="${o.src}" data-start="${num(s)}" data-duration="${num(d)}" data-media-start="${num(Math.max(0, ms))}" ` +
      `data-track-index="${tr}" data-volume="${vol}" data-fx-chain="${attr(fx)}"` + (auto ? ` data-automation="${auto}"` : "") + "></audio>";
    const out = [el(id, start, o.duration, mediaStart, track, voiceChain(null, 1, o.son, o.limit !== false), holes.length ? lane(points) : null)];
    const ends = [];  // fin de la dernière fenêtre de chaque piste (pistes après celle de la voix)
    wins.forEach((w, k) => {
      const fx = voiceChain(w.voice, w.force, o.son, o.limit !== false);
      const pitched = fx.nodes.some((n) => n.type === "pitchshift");
      const lead = Math.min((pitched ? PRE : 0) + XF, w.a - start);  // pas avant le début de la voix
      const s = w.a - lead, d = w.b - s + Math.min(XF, end - w.b);
      const ms = mediaStart + (s - start) + (pitched ? PITCH_LATENCY : 0);
      const fade = Math.min(XF, lead);
      const pts = [{ t: 0, v: 0 }, { t: lead - fade, v: 0 }, { t: lead, v: 1 }, { t: w.b - s, v: 1 }, { t: d, v: 0 }];
      let lane_ = ends.findIndex((x) => x <= s);
      if (lane_ < 0) lane_ = ends.push(0) - 1;
      ends[lane_] = s + d;
      out.push(el(`${id}-fx${k}`, s, d, ms, track + 1 + lane_, fx, lane(pts)));
    });
    return out.join("\n");
  }

  const api = { VOICES: Object.keys(VOICES), hasVoice, voiceChain, voiceElements, XF, PRE, PITCH_LATENCY };
  root.HFAudio = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof globalThis !== "undefined" ? globalThis : this);
