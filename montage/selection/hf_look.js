// Couleurs, effets d'image et transitions, partagés par le rendu (rust/src/render_hyperframes.rs :
// page HyperFrames, image par image) et l'aperçu de la page des moments : les deux appliquent les
// mêmes filtres CSS / SVG, calculés ici, donc l'aperçu ressemble à la vidéo entière.
// Listes reprises de FilmCraft (effets vidéo « Lumetri », « Image Control », « Stylize »… et
// transitions « Dissolve », « Wipe », « Slide »…), limitées à ce qu'un navigateur dessine seul.
(function (root) {
  "use strict";
  const clamp = (x, a, b) => Math.min(b, Math.max(a, x));
  const smooth = (a, b, x) => { const t = clamp((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t); };
  const r4 = (x) => String(Math.round(x * 1e4) / 1e4);
  const n = (v, d = 0) => (Number.isFinite(Number(v)) && v !== null && v !== "" ? Number(v) : d);

  // --- Retouche des couleurs (tout le montage, work/couleur.json) -------------------------------
  // Réglages de base façon Lumetri : balance des blancs (température, teinte), exposition (IL),
  // contraste, hautes lumières, ombres, blancs, noirs (-100 à 100), saturation (%), vibrance ;
  // un « look » (préréglage créatif) dosé par « intensite » (%) et un vignettage (%).
  const COLOR_DEFAULTS = { temperature: 0, teinte: 0, exposition: 0, contraste: 0, hautes: 0, ombres: 0, blancs: 0, noirs: 0,
    saturation: 100, vibrance: 0, look: "", intensite: 100, vignette: 0 };
  const COLOR_LIMITS = { temperature: [-100, 100], teinte: [-100, 100], exposition: [-3, 3], contraste: [-100, 100],
    hautes: [-100, 100], ombres: [-100, 100], blancs: [-100, 100], noirs: [-100, 100], saturation: [0, 200],
    vibrance: [-100, 100], intensite: [0, 100], vignette: [0, 100] };
  // Looks : écarts reportés sur les curseurs (saturation : points de % en plus ou en moins ;
  // withLook, look.rs LOOK_PARAMS) quand on choisit le look ou son intensité, et virage partiel
  // (split : couleur ajoutée aux ombres et aux hautes lumières, par canal R, V, B).
  const LOOKS = [
    { id: "chaud", label: "Chaud", p: { temperature: 30, saturation: 6, ombres: 5 } },
    { id: "froid", label: "Froid", p: { temperature: -30, teinte: -6 } },
    { id: "cinema", label: "Cinéma (sarcelle et orange)", p: { contraste: 18, saturation: -4, hautes: -8,
      split: { sh: [-0.05, 0.02, 0.07], hi: [0.07, 0.02, -0.05] } } },
    { id: "vintage", label: "Vintage", p: { noirs: 30, blancs: -12, saturation: -22, temperature: 18,
      split: { sh: [0.02, 0.0, 0.04], hi: [0.04, 0.03, -0.03] } } },
    { id: "delave", label: "Délavé", p: { noirs: 35, contraste: -18, saturation: -15 } },
    { id: "vif", label: "Vif", p: { vibrance: 45, contraste: 10, saturation: 8 } },
    { id: "nb", label: "Noir et blanc contrasté", p: { saturation: -100, contraste: 28, noirs: -10 } },
    { id: "nuit", label: "Nuit américaine", p: { exposition: -0.9, temperature: -45, saturation: -30, contraste: 10 } },
  ];

  function colorSettings(c) {
    const out = {};
    for (const [k, d] of Object.entries(COLOR_DEFAULTS)) {
      if (k === "look") out.look = LOOKS.some((l) => l.id === (c && c.look)) ? c.look : "";
      else out[k] = clamp(n(c && c[k], d), ...COLOR_LIMITS[k]);
    }
    if (c && c.v === 2) out.v = 2;  // look déjà sur les curseurs (look.rs)
    return out;
  }
  // Autre look ou autre intensité : l'écart de l'ancien look est retiré des curseurs, celui du
  // nouveau ajouté (comme look.rs with_look ; les retouches faites à la main restent).
  function withLook(c, id, intensite) {
    const s = colorSettings(c), old = LOOKS.find((l) => l.id === s.look), look = LOOKS.find((l) => l.id === id);
    const ko = old ? s.intensite / 100 : 0, kn = clamp(n(intensite, 100), 0, 100) / 100;
    for (const key of Object.keys(COLOR_LIMITS)) {
      if (key === "intensite") continue;
      s[key] = clamp(s[key] - ko * n(old && old.p[key]) + kn * n(look && look.p[key]), ...COLOR_LIMITS[key]);
    }
    return { ...s, look: look ? id : "", intensite: kn * 100, v: 2 };
  }
  // Réglages effectifs : curseurs, et virage partiel du look. Anciens réglages (sans v: 2) : look
  // ajouté aux curseurs, saturation multipliée.
  function colorParams(c) {
    const s = colorSettings(c), look = LOOKS.find((l) => l.id === s.look), k = look ? s.intensite / 100 : 0, p = {};
    const ka = s.v === 2 ? 0 : k;
    for (const key of ["temperature", "teinte", "exposition", "contraste", "hautes", "ombres", "blancs", "noirs", "vibrance"])
      p[key] = s[key] + ka * n(look && look.p[key]);
    p.sat = (s.saturation / 100) * Math.max(0, 1 + ka * n(look && look.p.saturation) / 100);
    const sp = (look && look.p.split) || { sh: [0, 0, 0], hi: [0, 0, 0] };
    p.split = { sh: sp.sh.map((v) => v * k), hi: sp.hi.map((v) => v * k) };
    return p;
  }
  // Courbe d'un canal (0 = R, 1 = V, 2 = B) : x de 0 à 1 → valeur de sortie.
  function channelCurve(p, ch) {
    const t = p.temperature / 100, ti = p.teinte / 100;
    const gain = Math.pow(2, p.exposition) * [1 + 0.18 * t + 0.04 * ti, 1 - 0.12 * ti, 1 - 0.18 * t + 0.04 * ti][ch];
    const bl = -0.15 * p.noirs / 100, wh = 1 - 0.15 * p.blancs / 100, c = p.contraste / 100;
    return (x0) => {
      let x = x0 * gain;
      x = (x - bl) / Math.max(0.05, wh - bl);
      // Ombres et hautes lumières : bosses centrées sur 1/3 et 2/3 (le noir et le blanc purs bougent peu).
      x += 0.25 * (p.ombres / 100) * 6.75 * x * (1 - x) * (1 - x) * (x > 0 && x < 1 ? 1 : 0);
      x += 0.25 * (p.hautes / 100) * 6.75 * x * x * (1 - x) * (x > 0 && x < 1 ? 1 : 0);
      x = clamp(x, 0, 1);
      // Contraste : en S autour du gris moyen (positif), aplati vers le gris (négatif).
      x = c >= 0 ? x + c * (x * x * (3 - 2 * x) - x) * 1.6 : 0.5 + (x - 0.5) * (1 + c * 0.8);
      x += p.split.sh[ch] * (1 - x) * (1 - x) + p.split.hi[ch] * x * x;
      return clamp(x, 0, 1);
    };
  }
  const SAT_L = [0.2126, 0.7152, 0.0722];
  function satMatrix(s) {
    const rows = [0, 1, 2].map((i) => [0, 1, 2].map((j) => (1 - s) * SAT_L[j] + (i === j ? s : 0)).concat([0, 0]));
    return rows.concat([[0, 0, 0, 1, 0]]).map((r) => r.map(r4).join(" ")).join(" ");
  }
  const isNeutral = (s) => Object.entries(COLOR_DEFAULTS).every(([k, d]) => k === "vignette" || (k === "intensite") || (k === "look" ? !s.look : Math.abs(s[k] - d) < 1e-6));
  // Filtre SVG des couleurs (id « am-couleur ») et vignettage : { svg, css, vignette } ;
  // svg vide si tout est neutre (pas de filtre, rien à calculer à chaque image).
  function colorFilter(c) {
    const s = colorSettings(c);
    const vignette = s.vignette > 0
      ? `radial-gradient(ellipse at 50% 50%, rgba(0,0,0,0) ${r4(60 - 25 * s.vignette / 100)}%, rgba(0,0,0,${r4(0.85 * s.vignette / 100)}) 100%)` : "";
    if (isNeutral(s)) return { svg: "", css: "", vignette };
    const p = colorParams(s), N = 128;
    const tables = [0, 1, 2].map((ch) => { const f = channelCurve(p, ch); return Array.from({ length: N + 1 }, (_, i) => r4(f(i / N))).join(" "); });
    const vib = 1 + 0.45 * p.vibrance / 100;  // vibrance : saturation douce (approchée, sans masque des tons chair)
    const svg = `<filter id="am-couleur" x="0" y="0" width="100%" height="100%" color-interpolation-filters="sRGB">` +
      `<feComponentTransfer><feFuncR type="table" tableValues="${tables[0]}"/><feFuncG type="table" tableValues="${tables[1]}"/>` +
      `<feFuncB type="table" tableValues="${tables[2]}"/></feComponentTransfer>` +
      `<feColorMatrix type="matrix" values="${satMatrix(p.sat * vib)}"/></filter>`;
    return { svg, css: "url(#am-couleur)", vignette };
  }

  // --- Effets d'image d'un moment (effet visuel « filtre », champ « effect ») -----------------
  // force de 0 à 100 ; W : largeur de l'image en px (tailles relatives à 1080 px de large).
  const EFFECTS = [
    { id: "noirblanc", label: "Noir et blanc", force: 100 },
    { id: "sepia", label: "Sépia", force: 80 },
    { id: "negatif", label: "Négatif", force: 100 },
    { id: "teinte", label: "Teinte (bleu nuit et crème)", force: 80 },
    { id: "flou", label: "Flou", force: 40 },
    { id: "floudir", label: "Flou de mouvement", force: 50 },
    { id: "nettete", label: "Netteté", force: 50 },
    { id: "lueur", label: "Lueur", force: 50 },
    { id: "posteriser", label: "Postérisation", force: 60 },
    { id: "mosaique", label: "Mosaïque (pixels)", force: 50 },
    { id: "contours", label: "Contours (dessin)", force: 100 },
    { id: "bruit", label: "Grain", force: 40 },
    { id: "vhs", label: "VHS (couleurs décalées, lignes)", force: 50 },
    { id: "stroboscope", label: "Stroboscope", force: 50 },
    { id: "miroir", label: "Miroir (image retournée)", force: 100 },
    { id: "envers", label: "À l'envers", force: 100 },
  ];
  const RAMP = 0.12;  // montée et descente de l'effet (s)
  // Effet actif à l'instant t (secondes depuis son début, d : sa durée) : { css, svg, over, flip }.
  //   css : fonctions de filtre CSS ; svg : <filter id="am-fx"> si l'effet en a besoin (css le
  //   référence) ; over : style d'un calque posé sur l'image (grain, lignes, éclair) ;
  //   flip : [sx, sy] image retournée.
  function effectLayers(id, force, t, d, W = 1080, frame = 0) {
    const env = d > 2 * RAMP ? Math.min(1, t / RAMP, (d - t) / RAMP) : 1;
    const f = clamp(n(force, 50) / 100, 0, 1.5) * clamp(env, 0, 1), u = W / 1080;
    const out = { css: "", svg: "", over: "", flip: null };
    const svg = (body, region = 'x="0" y="0" width="100%" height="100%"') => {
      out.svg = `<filter id="am-fx" ${region} color-interpolation-filters="sRGB">${body}</filter>`;
      out.css = "url(#am-fx)";
    };
    if (f <= 0.001 && !["miroir", "envers"].includes(id)) return out;
    switch (id) {
      case "noirblanc": out.css = `grayscale(${r4(Math.min(1, f))})`; break;
      case "sepia": out.css = `sepia(${r4(Math.min(1, f))})`; break;
      case "negatif": out.css = `invert(${r4(Math.min(1, f))})`; break;
      case "flou": out.css = `blur(${r4(f * 16 * u)}px)`; break;
      case "floudir": svg(`<feGaussianBlur stdDeviation="${r4(f * 28 * u)} 0" edgeMode="duplicate"/>`); break;
      case "nettete": { const a = r4(f * 1.2); svg(`<feConvolveMatrix order="3" preserveAlpha="true" edgeMode="duplicate" kernelMatrix="0 -${a} 0 -${a} ${r4(1 + 4 * f * 1.2)} -${a} 0 -${a} 0"/>`); break; }
      case "lueur": svg(`<feGaussianBlur in="SourceGraphic" stdDeviation="${r4(14 * u)}" result="b"/>` +
        `<feComponentTransfer in="b" result="g"><feFuncR type="linear" slope="${r4(0.6 + f)}" intercept="-0.15"/><feFuncG type="linear" slope="${r4(0.6 + f)}" intercept="-0.15"/><feFuncB type="linear" slope="${r4(0.6 + f)}" intercept="-0.15"/></feComponentTransfer>` +
        `<feBlend in="SourceGraphic" in2="g" mode="screen"/>`); break;
      case "posteriser": { const k = Math.round(clamp(12 - 10 * f, 2, 12)); const v = Array.from({ length: k }, (_, i) => r4(i / (k - 1))).join(" ");
        svg(`<feComponentTransfer><feFuncR type="discrete" tableValues="${v}"/><feFuncG type="discrete" tableValues="${v}"/><feFuncB type="discrete" tableValues="${v}"/></feComponentTransfer>`); break; }
      case "mosaique": { const s = Math.max(2, Math.round(4 + f * 44 * u)), h = Math.max(1, Math.floor(s / 2));
        svg(`<feFlood x="${h - 1}" y="${h - 1}" width="2" height="2"/><feComposite width="${s}" height="${s}"/><feTile result="a"/>` +
          `<feComposite in="SourceGraphic" in2="a" operator="in"/><feMorphology operator="dilate" radius="${h}"/>`); break; }
      case "contours": svg(`<feColorMatrix type="saturate" values="0" result="g"/>` +
        `<feConvolveMatrix in="g" order="3" preserveAlpha="true" edgeMode="duplicate" kernelMatrix="-1 -1 -1 -1 8 -1 -1 -1 -1" result="e"/>` +
        `<feComponentTransfer in="e" result="i"><feFuncR type="linear" slope="-2.5" intercept="1"/><feFuncG type="linear" slope="-2.5" intercept="1"/><feFuncB type="linear" slope="-2.5" intercept="1"/></feComponentTransfer>` +
        `<feComposite in="i" in2="SourceGraphic" operator="arithmetic" k2="${r4(Math.min(1, f))}" k3="${r4(1 - Math.min(1, f))}"/>`); break;
      case "bruit": svg(`<feTurbulence type="fractalNoise" baseFrequency="${r4(0.9 / u)}" numOctaves="1" seed="${(frame % 997) + 1}" result="t"/>` +
        `<feColorMatrix in="t" type="matrix" values="0.33 0.33 0.33 0 0 0.33 0.33 0.33 0 0 0.33 0.33 0.33 0 0 0 0 0 0 1" result="n"/>` +
        `<feComposite in="SourceGraphic" in2="n" operator="arithmetic" k2="1" k3="${r4(0.5 * f)}" k4="${r4(-0.25 * f)}"/>`); break;
      case "vhs": { const dx = r4((3 + 10 * f) * u * (1 + 0.5 * Math.sin(t * 23))), y = r4(Math.sin(t * 7) * 2 * u);
        svg(`<feColorMatrix in="SourceGraphic" type="matrix" values="1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0" result="r"/>` +
          `<feOffset in="r" dx="${dx}" dy="${y}" result="ro"/>` +
          `<feColorMatrix in="SourceGraphic" type="matrix" values="0 0 0 0 0 0 1 0 0 0 0 0 1 0 0 0 0 0 1 0" result="c"/>` +
          `<feOffset in="c" dx="-${dx}" result="co"/><feBlend in="ro" in2="co" mode="screen"/>`);
        out.over = `background: repeating-linear-gradient(to bottom, rgba(0,0,0,${r4(0.28 * Math.min(1, f))}) 0px, rgba(0,0,0,${r4(0.28 * Math.min(1, f))}) ${r4(2 * u)}px, rgba(0,0,0,0) ${r4(2 * u)}px, rgba(0,0,0,0) ${r4(5 * u)}px);`;
        break; }
      case "teinte": svg(`<feColorMatrix type="matrix" values="0.2126 0.7152 0.0722 0 0 0.2126 0.7152 0.0722 0 0 0.2126 0.7152 0.0722 0 0 0 0 0 1 0" result="l"/>` +
        `<feComponentTransfer in="l" result="d"><feFuncR type="table" tableValues="0.06 1"/><feFuncG type="table" tableValues="0.12 0.9"/><feFuncB type="table" tableValues="0.26 0.72"/></feComponentTransfer>` +
        `<feComposite in="d" in2="SourceGraphic" operator="arithmetic" k2="${r4(Math.min(1, f))}" k3="${r4(1 - Math.min(1, f))}"/>`); break;
      case "stroboscope": { const hz = 4 + 8 * Math.min(1, f); if (Math.floor(t * hz * 2) % 2 === 0) out.over = `background: #fff; opacity: ${r4(0.85 * env)};`; break; }
      case "miroir": out.flip = [-1, 1]; break;
      case "envers": out.flip = [1, -1]; break;
    }
    return out;
  }

  // --- Transitions entre deux moments (work/transitions.json, ou réglage d'un moment) ----------
  // La transition commence à la coupe : le plan entrant joue à sa place (voix et image calées),
  // le plan sortant continue pendant « dur » secondes au-dessous (dans l'aperçu de la page :
  // sa dernière image, figée). p de 0 à 1 ; W, H : taille de l'image en px.
  const TRANSITIONS = [
    { id: "fondu", label: "Fondu enchaîné" },
    { id: "fondunoir", label: "Fondu au noir" },
    { id: "fondublanc", label: "Fondu au blanc" },
    { id: "fonduflou", label: "Fondu flou" },
    { id: "fonduadditif", label: "Fondu additif (lumineux)" },
    { id: "fondufilm", label: "Fondu film" },
    { id: "balayage", label: "Balayage" },
    { id: "balayagedoux", label: "Balayage doux" },
    { id: "volets", label: "Volets" },
    { id: "horloge", label: "Horloge" },
    { id: "iris", label: "Iris (cercle)" },
    { id: "losange", label: "Losange" },
    { id: "ouverture", label: "Ouverture (depuis le centre)" },
    { id: "poussee", label: "Poussée" },
    { id: "glissement", label: "Glissement" },
    { id: "etirement", label: "Étirement" },
    { id: "fouet", label: "Coup de fouet" },
    { id: "zoomcroise", label: "Zoom croisé" },
    { id: "flash", label: "Éclair" },
    { id: "seisme", label: "Séisme" },
    { id: "glitch", label: "Glitch" },
    { id: "retournement", label: "Retournement" },
    { id: "tele", label: "Télé qui s'éteint" },
  ];
  const TRANSITION_DUR = { min: 0.2, max: 1.5, def: 0.5 };
  const ez = (p) => (p < 0.5 ? 2 * p * p : 1 - Math.pow(-2 * p + 2, 2) / 2);
  const hash = (k) => { const x = Math.sin(k * 127.1 + 311.7) * 43758.5453; return x - Math.floor(x); };
  // Styles des deux plans et d'un calque de couleur : { out, in, over } ; chaque style : opacity,
  // transform, filter, clipPath, mask, blend (mix-blend-mode), z (« in » au-dessus par défaut).
  function transitionStyles(id, p0, W = 1080, H = 1920) {
    const p = clamp(p0, 0, 1), e = ez(p), u = W / 1080;
    const o = {}, i = {}, res = { out: o, in: i, over: null };
    const pct = (x) => `${r4(x * 100)}%`;
    switch (id) {
      case "fondu": i.opacity = p; break;
      case "fondunoir": case "fondublanc":
        res.over = { background: id === "fondunoir" ? "#000" : "#fff", opacity: 1 - Math.abs(2 * p - 1) };
        i.opacity = p >= 0.5 ? 1 : 0; break;
      case "fonduflou": o.filter = `blur(${r4(e * 24 * u)}px)`; i.filter = `blur(${r4((1 - e) * 24 * u)}px)`; i.opacity = e; break;
      case "fonduadditif": o.filter = `brightness(${r4(1 - Math.max(0, 2 * p - 1))})`; i.blend = "plus-lighter"; i.opacity = Math.min(1, 2 * p); break;
      case "fondufilm": { const b = r4(1 + 0.35 * Math.sin(Math.PI * p)); o.filter = `brightness(${b})`; i.filter = `brightness(${b})`; i.opacity = smooth(0.15, 0.85, p); break; }
      case "balayage": i.clipPath = `inset(0 ${pct(1 - e)} 0 0)`; break;
      case "balayagedoux": { const a = e * 130 - 30; i.mask = `linear-gradient(to right, #000 ${r4(a)}%, rgba(0,0,0,0) ${r4(a + 30)}%)`; break; }
      case "volets": { const bar = H / 8, w = e * bar; i.mask = `repeating-linear-gradient(to bottom, #000 0px, #000 ${r4(w)}px, rgba(0,0,0,0) ${r4(w)}px, rgba(0,0,0,0) ${r4(bar)}px)`; break; }
      case "horloge": i.mask = `conic-gradient(from 0deg at 50% 50%, #000 ${r4(e * 360)}deg, rgba(0,0,0,0) ${r4(e * 360)}deg)`; break;
      case "iris": i.clipPath = `circle(${pct(e * 0.72)} at 50% 50%)`; break;
      case "losange": { const k = e * 1.0; i.clipPath = `polygon(50% ${pct(0.5 - k)}, ${pct(0.5 + k)} 50%, 50% ${pct(0.5 + k)}, ${pct(0.5 - k)} 50%)`; break; }
      case "ouverture": i.clipPath = `inset(0 ${pct((1 - e) / 2)} 0 ${pct((1 - e) / 2)})`; break;
      case "poussee": o.transform = `translateX(${pct(-e)})`; i.transform = `translateX(${pct(1 - e)})`; break;
      case "glissement": i.transform = `translateX(${pct(1 - e)})`; break;
      case "etirement": o.transform = `scaleX(${r4(Math.max(0.001, 1 - e))})`; o.origin = "100% 50%"; i.transform = `scaleX(${r4(Math.max(0.001, e))})`; i.origin = "0% 50%"; break;
      case "fouet": { const b = r4(Math.sin(Math.PI * p) * 30 * u); o.transform = `translateX(${pct(-e)})`; i.transform = `translateX(${pct(1 - e)})`; o.filter = `blur(${b}px)`; i.filter = `blur(${b}px)`; break; }
      case "zoomcroise": o.transform = `scale(${r4(1 + 2 * e)})`; o.filter = `blur(${r4(e * 20 * u)}px)`;
        i.transform = `scale(${r4(3 - 2 * e)})`; i.filter = `blur(${r4((1 - e) * 20 * u)}px)`; i.opacity = smooth(0.35, 0.65, p); break;
      case "flash": res.over = { background: "#fff", opacity: Math.pow(1 - Math.abs(2 * p - 1), 0.6) }; i.opacity = p >= 0.5 ? 1 : 0; break;
      case "seisme": { const a = Math.sin(Math.PI * p) * 40 * u, tr = `translate(${r4(a * Math.sin(p * 91))}px, ${r4(a * Math.cos(p * 73))}px)`; o.transform = tr; i.transform = tr; i.opacity = smooth(0.3, 0.7, p); break; }
      case "glitch": { const k = Math.floor(p * 14), on = p > 0.85 || (p > 0.2 && hash(k) < p);
        i.opacity = on ? 1 : 0;
        if (p < 0.9) { const y0 = hash(k + 7) * 80, h = 6 + hash(k + 3) * 18, dx = (hash(k + 11) - 0.5) * 60 * u;
          (on ? o : i).clipPath = `inset(${r4(y0)}% 0 ${r4(Math.max(0, 100 - y0 - h))}% 0)`; (on ? o : i).transform = `translateX(${r4(dx)}px)`;
          (on ? o : i).opacity = 1; (on ? o : i).z = 2; (on ? i : o).filter = `hue-rotate(${Math.round(hash(k + 5) * 180)}deg)`; }
        break; }
      case "retournement": if (p < 0.5) { o.transform = `perspective(${r4(2 * W)}px) rotateY(${r4(e * 180)}deg)`; i.opacity = 0; }
        else { i.transform = `perspective(${r4(2 * W)}px) rotateY(${r4((e - 1) * 180)}deg)`; o.opacity = 0; }
        break;
      case "tele": if (p < 0.5) { const q = p * 2; o.transform = q < 0.6 ? `scaleY(${r4(Math.max(0.004, 1 - q / 0.6))})` : `scale(${r4(Math.max(0.001, 1 - (q - 0.6) / 0.4))}, 0.004)`;
          o.filter = `brightness(${r4(1 + 2 * q)})`; i.opacity = 0; }
        else { const q = (1 - p) * 2; i.transform = q < 0.6 ? `scaleY(${r4(Math.max(0.004, 1 - q / 0.6))})` : `scale(${r4(Math.max(0.001, 1 - (q - 0.6) / 0.4))}, 0.004)`;
          i.filter = `brightness(${r4(1 + 2 * q)})`; o.opacity = 0; }
        break;  // fond noir de l'image autour du plan qui se replie
      default: i.opacity = p;
    }
    return res;
  }
  // Style d'un plan appliqué à un élément (null : style retiré).
  function applyStyle(el, s) {
    const st = el.style;
    st.opacity = s && s.opacity !== undefined ? r4(s.opacity) : "";
    st.transform = (s && s.transform) || "";
    st.transformOrigin = (s && s.origin) || "";
    st.filter = (s && s.filter) || "";
    st.clipPath = (s && s.clipPath) || "";
    st.webkitMaskImage = st.maskImage = (s && s.mask) || "";
    st.mixBlendMode = (s && s.blend) || "";
    st.zIndex = s && s.z ? String(s.z) : "";
  }
  const hasTransition = (id) => TRANSITIONS.some((x) => x.id === id);
  const hasEffect = (id) => EFFECTS.some((x) => x.id === id);

  const api = { COLOR_DEFAULTS, COLOR_LIMITS, LOOKS, EFFECTS, TRANSITIONS, TRANSITION_DUR, colorSettings, withLook, colorFilter,
    effectLayers, transitionStyles, applyStyle, hasTransition, hasEffect, channelCurve, colorParams };
  root.HFLook = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof globalThis !== "undefined" ? globalThis : this);
