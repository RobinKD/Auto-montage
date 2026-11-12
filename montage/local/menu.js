// Bandeau commun aux pages de l'interface locale : nom d'Auto-montage, page en cours, et un
// menu déroulant vers les autres pages (moments, rushes, téléchargements, Claude, mises à
// jour, journal). Ajouté par rust/src/local_server.rs à toutes les pages ; les pages publiées
// sur claude.ai ne l'ont pas. Couleurs : celles de la page (variables --bg, --accent…).
// Mode du montage en cours (work/mode.txt, écrit par le serveur sur <html data-mode>) :
// « Dérushage » pour un nouveau rush (couper, choisir les moments, corriger les sous-titres),
// « Auto-montage » une fois le dérush validé (page des moments) ; en dérushage, les parties
// marquées data-mode-montage sont masquées. Bouton « Raccourcis », à gauche du menu, sur les
// pages qui en ont (page des moments : window.AM_RACCOURCIS).
// Choix du mode en tête du menu déroulé (PUT /api/mode : mode du montage en cours).
(() => {
  const MODE = document.documentElement.dataset.mode === "derush" ? "derush" : "montage";
  const DERUSH = MODE === "derush";
  // [adresse, nom, description, description en dérushage (null : page absente en dérushage)]
  const PAGES = [
    ["/", "Page des moments", "Choisir les moments, effets, découpe, génération", "Choisir et découper les moments, générer"],
    ["/rush/", "Rushes et montages", "Nouveau rush, consignes, montages enregistrés", "Nouveau rush, consignes, montages enregistrés"],
    ["/downloads/", "Téléchargements", "Vidéo entière en 720p, 1080p, 4K"],
    ["/chat/", "Discuter avec Claude", "Changer le montage en lui écrivant", null],
    ["/claude/", "Connexion à Claude", "Relier votre compte claude.ai", "Relier votre compte claude.ai (consignes pour choisir les moments)"],
    ["/updates/", "Mises à jour", "Nouvelle version d'Auto-montage"],
    ["/logs/", "Journal", "En cas de problème : ce qui s'est passé"],
    ["/tutoriel/", "Tutoriel", "Comment utiliser Auto-montage, avec ou sans Claude"],
  ];
  const here = location.pathname.replace(/\/?$/, "/").replace(/^\/index\.html\/$/, "/");
  const current = PAGES.find(([href]) => href === here);

  const css = `
  .am-bar { position: sticky; top: 0; z-index: 50; background: var(--surface, #fffdf9); border-bottom: 1px solid var(--line, #e4ded4);
    font-family: var(--body, "Source Sans 3", "Helvetica Neue", Arial, sans-serif); color: var(--fg, #1f1d1a); }
  .am-bar-in { max-width: 1100px; margin: 0 auto; padding: 8px 16px; display: flex; align-items: center; gap: 12px; }
  .am-brand { font-family: var(--display, Poppins, sans-serif); font-weight: 900; font-size: 16px; color: inherit; text-decoration: none; white-space: nowrap; }
  .am-here { font-size: 14px; color: var(--muted, #6f695f); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .am-toggle { margin-left: auto; display: inline-flex; align-items: center; gap: 8px; font: inherit; font-size: 15px; font-weight: 600;
    color: var(--fg, #1f1d1a); background: var(--bg, #f5f3ef); border: 1px solid var(--line, #e4ded4); border-radius: 8px; padding: 6px 12px; cursor: pointer; flex: none; }
  .am-toggle .am-chev { transition: transform 150ms; font-size: 12px; }
  .am-toggle[aria-expanded="true"] .am-chev { transform: rotate(180deg); }
  .am-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--warn, #9a5b12); }
  .am-panel { border-top: 1px solid var(--line, #e4ded4); background: var(--surface, #fffdf9); box-shadow: 0 8px 18px rgb(0 0 0 / 0.08); }
  .am-panel ul { list-style: none; margin: 0 auto; max-width: 1100px; padding: 8px 16px 12px;
    display: grid; grid-template-columns: repeat(auto-fill, minmax(230px, 1fr)); gap: 6px; }
  .am-panel li { list-style: none; margin: 0; padding: 0; border: 0; border-radius: 0; background: none; box-shadow: none; display: block; min-width: 0; }
  .am-panel a { display: flex; flex-direction: column; gap: 1px; padding: 8px 10px; border-radius: 8px; text-decoration: none; color: var(--fg, #1f1d1a); border: 1px solid transparent; }
  .am-panel a:hover { background: var(--bg, #f5f3ef); border-color: var(--line, #e4ded4); }
  .am-panel a[aria-current="page"] { background: var(--accent-soft, #dcebe5); }
  .am-panel b { font-weight: 600; font-size: 15px; display: flex; align-items: center; gap: 8px; }
  .am-panel small { font-size: 13px; color: var(--muted, #6f695f); }
  .am-panel .am-badge { font-size: 11px; font-weight: 600; color: var(--warn, #9a5b12); background: var(--warn-soft, #f6e7d2); border-radius: 999px; padding: 0 7px; }
  /* Parties qui ont besoin de Claude Code (attribut data-needs-claude) : pastille en haut à
     droite ; grisées et inutilisables tant que Claude Code n'est pas connecté. */
  [data-needs-claude] { position: relative; }
  .am-needs { position: absolute; top: 8px; right: 10px; z-index: 2; font: 600 12px/1.6 var(--body, sans-serif);
    border-radius: 999px; padding: 1px 10px; background: var(--accent-soft, #dcebe5); color: var(--accent, #2d6a5a);
    text-decoration: none; white-space: nowrap; }
  .am-claude-off > .am-needs { background: var(--warn-soft, #f6e7d2); color: var(--warn, #9a5b12); }
  .am-claude-off > :not(.am-needs) { opacity: 0.45; filter: grayscale(1); }
  .am-panel a.am-off { opacity: 0.5; cursor: not-allowed; }
  .am-panel .am-badge.am-cc { color: var(--accent, #2d6a5a); background: var(--accent-soft, #dcebe5); }
  .am-keys { margin-left: auto; font: inherit; font-size: 15px; font-weight: 600; color: var(--fg, #1f1d1a); background: none;
    border: 1px solid var(--line, #e4ded4); border-radius: 8px; padding: 6px 12px; cursor: pointer; flex: none; white-space: nowrap; }
  .am-keys:hover { background: var(--bg, #f5f3ef); }
  .am-keys[hidden] { display: none; }
  .am-keys:not([hidden]) + .am-toggle { margin-left: 0; }
  .am-panel-mode { max-width: 1100px; margin: 0 auto; padding: 10px 16px 0; display: flex; align-items: center; gap: 10px; font-size: 14px; font-weight: 600; color: var(--muted, #6f695f); }
  .am-mode { display: inline-flex; flex: none; padding: 2px; gap: 2px; border-radius: 9px; background: var(--bg, #f5f3ef); border: 1px solid var(--line, #e4ded4); }
  .am-mode button { font: inherit; font-size: 14px; font-weight: 600; color: var(--muted, #6f695f); background: none; border: 0; border-radius: 7px; padding: 5px 12px; cursor: pointer; white-space: nowrap; }
  .am-mode button:hover { color: var(--fg, #1f1d1a); }
  .am-mode button[aria-checked="true"] { background: var(--accent, #2d6a5a); color: #fff; cursor: default; }
  .am-mode button:disabled:not([aria-checked="true"]) { opacity: 0.5; cursor: not-allowed; }
  .am-mode-note { font-weight: 400; font-size: 13px; }
  .am-off-page { max-width: 640px; margin: 48px auto; padding: 20px 22px; border: 1px solid var(--line, #e4ded4); border-radius: 12px;
    background: var(--surface, #fffdf9); font-family: var(--body, sans-serif); color: var(--fg, #1f1d1a); }
  .am-off-page h1 { font-size: 20px; margin: 0 0 8px; }
  .am-off-page p { margin: 0 0 12px; color: var(--muted, #6f695f); }
  .am-off-page button { font: inherit; font-weight: 600; color: #fff; background: var(--accent, #2d6a5a); border: 0; border-radius: 8px; padding: 8px 14px; cursor: pointer; }
  @media (max-width: 640px) { .am-here { display: none; } .am-brand { display: none; } .am-keys { padding: 6px 9px; } }
  /* Fenêtre de confirmation commune (window.amConfirm), à la place de confirm() du navigateur. */
  .am-ask { max-width: min(520px, calc(100vw - 32px)); border: 1px solid var(--line, #e4ded4); border-radius: 12px; padding: 18px 20px;
    background: var(--surface, #fffdf9); color: var(--fg, #1f1d1a); font-family: var(--body, sans-serif); font-size: 15px; line-height: 1.45;
    box-shadow: 0 16px 40px rgb(0 0 0 / 0.25); }
  .am-ask::backdrop { background: rgb(0 0 0 / 0.45); }
  .am-ask h2 { font: 700 17px/1.3 var(--display, sans-serif); margin: 0 0 8px; }
  .am-ask p { margin: 0 0 8px; white-space: pre-line; }
  .am-ask-check { display: flex; gap: 8px; align-items: flex-start; margin: 4px 0 0; font-weight: 600; cursor: pointer; }
  .am-ask-check input { margin-top: 4px; accent-color: var(--accent, #2d6a5a); }
  .am-ask-foot { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; flex-wrap: wrap; }
  .am-ask-foot button { font: inherit; font-weight: 600; border-radius: 8px; padding: 7px 14px; cursor: pointer;
    border: 1px solid var(--line, #e4ded4); background: var(--bg, #f5f3ef); color: var(--fg, #1f1d1a); }
  .am-ask-foot .am-ask-ok { background: var(--accent, #2d6a5a); border-color: var(--accent, #2d6a5a); color: #fff; }
  .am-ask-foot .am-ask-ok.am-danger { background: var(--warn, #9a5b12); border-color: var(--warn, #9a5b12); }
  .am-bar :focus-visible { outline: 2px solid var(--accent, #2d6a5a); outline-offset: 2px; }
  @media (prefers-reduced-motion: reduce) { .am-toggle .am-chev { transition: none; } }`;

  function build() {
    const style = document.createElement("style");
    style.textContent = css;
    document.head.append(style);
    const bar = document.createElement("div");
    bar.className = "am-bar";
    bar.innerHTML = `<div class="am-bar-in">
        <a class="am-brand" href="/">Auto-montage</a><span class="am-here"></span>
        <button type="button" class="am-keys" hidden title="Raccourcis clavier de la page, à voir et à changer">Raccourcis</button>
        <button type="button" class="am-toggle" aria-expanded="false" aria-controls="am-panel">
          <span class="am-dot" hidden></span>Menu<span class="am-chev" aria-hidden="true">▾</span></button>
      </div>
      <div class="am-panel" id="am-panel" role="navigation" aria-label="Pages d'Auto-montage" hidden>
        <div class="am-panel-mode"><span>Mode</span>
          <div class="am-mode" role="radiogroup" aria-label="Mode">
            <button type="button" role="radio" data-mode="derush" title="Découper le rush en moments, sans effets ; Claude peut choisir les moments d'après des consignes">Dérushage</button>
            <button type="button" role="radio" data-mode="montage" title="Toutes les fonctions : effets, sous-titres, consignes et Claude">Auto-montage</button>
          </div><small class="am-mode-note"></small></div><ul></ul></div>`;
    const toggle = bar.querySelector(".am-toggle");
    const panel = bar.querySelector(".am-panel");
    bar.querySelector(".am-here").textContent = (current ? current[1] : "") + (current && current[0] === "/" ? (DERUSH ? " · dérushage" : " · auto-montage") : "");
    // Raccourcis clavier : seulement sur une page qui en a (la page des moments les déclare).
    const keys = bar.querySelector(".am-keys");
    const showKeys = () => { keys.hidden = !window.AM_RACCOURCIS; };
    keys.addEventListener("click", () => { if (window.AM_RACCOURCIS) window.AM_RACCOURCIS.open(); });
    showKeys();
    document.addEventListener("am-raccourcis", showKeys);
    for (const btn of bar.querySelectorAll(".am-mode button")) {
      btn.setAttribute("aria-checked", String(btn.dataset.mode === MODE));
      btn.addEventListener("click", () => setMode(btn.dataset.mode, bar));
    }
    for (const [href, label, montageHint, derushHint] of PAGES) {
      if (DERUSH && derushHint === null) continue;
      const hint = DERUSH && derushHint ? derushHint : montageHint;
      const li = document.createElement("li");
      const a = Object.assign(document.createElement("a"), { href });
      if (current && current[0] === href) a.setAttribute("aria-current", "page");
      a.innerHTML = "<b></b><small></small>";
      a.querySelector("b").textContent = label;
      a.querySelector("small").textContent = hint;
      a.dataset.href = href;
      li.append(a);
      panel.querySelector("ul").append(li);
    }
    // Menu ouvert : état relu (une tâche a pu commencer ou finir depuis le chargement de la page).
    const open = (yes) => {
      panel.hidden = !yes;
      toggle.setAttribute("aria-expanded", String(yes));
      if (yes) fetch("/api/status").then((r) => r.json()).then((st) => markMode(bar, st)).catch(() => {});
    };
    toggle.addEventListener("click", () => open(panel.hidden));
    document.addEventListener("click", (e) => { if (!bar.contains(e.target)) open(false); });
    document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !panel.hidden) { open(false); toggle.focus(); } });
    document.body.prepend(bar);
    if (DERUSH && current && current[3] === null) offPage();
    // Bandeau de bord à bord, malgré la marge intérieure de la page.
    const pad = getComputedStyle(document.body);
    bar.style.marginLeft = `-${pad.paddingLeft}`;
    bar.style.marginRight = `-${pad.paddingRight}`;
    // Hauteur du bandeau (sans le menu déroulé) dans --am-top : les parties figées des pages
    // (lecteur et boutons de la page des moments) se placent dessous au lieu d'être cachées.
    const row = bar.querySelector(".am-bar-in");
    const setTop = () => document.documentElement.style.setProperty("--am-top", `${row.offsetHeight + 1}px`);
    setTop();
    if (window.ResizeObserver) new ResizeObserver(setTop).observe(row);

    // Nouvelle version disponible : pastille sur le menu et sur « Mises à jour ».
    fetch("/api/status").then((r) => r.json()).then((st) => {
      markClaude(!!st.claude);
      markMode(bar, st);
      // Premier lancement : le tutoriel d'abord (une seule fois, il le note en s'ouvrant).
      if (st.tutorialSeen === false && here !== "/tutoriel/") { location.href = "/tutoriel/?bienvenue=1"; return; }
      if (!st.update || !st.update.newer) return;
      bar.querySelector(".am-dot").hidden = false;
      const b = panel.querySelector('a[data-href="/updates/"] b');
      const badge = Object.assign(document.createElement("span"), { className: "am-badge", textContent: `${st.update.latest} disponible` });
      b.append(badge);
    }).catch(() => {});
  }
  // Changement de mode du montage en cours : enregistré par le serveur (work/mode.txt), puis la page
  // est rechargée pour montrer ou masquer ses parties.
  async function setMode(mode, bar) {
    if (mode === MODE) return;
    const buttons = bar.querySelectorAll(".am-mode button");
    buttons.forEach((b) => { b.disabled = true; });
    const r = await fetch("/api/mode", { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ mode }) }).catch(() => null);
    if (r && r.ok) { location.reload(); return; }
    buttons.forEach((b) => { b.disabled = false; });
    const err = r ? (await r.json().catch(() => ({}))).error : null;
    const note = bar.querySelector(".am-panel-mode .am-mode-note");
    note.textContent = `Le mode n'a pas pu être changé : ${err || "Auto-montage ne répond pas."}`;
  }
  // Mode : pas de changement pendant une tâche (préparation, génération, passage à l'auto-montage…,
  // qui lisent le mode) ; sans montage en cours, il décide de l'usage des consignes du prochain rush.
  function markMode(bar, st) {
    const job = st.job || {};
    const running = job.state === "running";
    const note = bar.querySelector(".am-panel-mode .am-mode-note");
    for (const btn of bar.querySelectorAll(".am-mode button")) {
      if (btn.dataset.title === undefined) btn.dataset.title = btn.title;
      btn.disabled = running && btn.dataset.mode !== MODE;
      btn.title = running ? "Une tâche est en cours : changez de mode quand elle sera finie" : btn.dataset.title;
    }
    note.textContent = running ? "pas pendant une tâche"
      : st.montage ? "" : "aucun montage en cours : choisit l'usage des consignes du prochain rush";
  }
  // Page réservée au mode Auto-montage (Claude), ouverte en dérushage : remplacée par un message.
  function offPage() {
    for (const el of document.body.children) if (!el.classList.contains("am-bar")) el.hidden = true;
    const box = document.createElement("section");
    box.className = "am-off-page";
    box.innerHTML = "<h1></h1><p>Ce montage est en dérushage : couper le rush en moments, les choisir et corriger les sous-titres, sans Claude ni effets. Cette page sert à l'auto-montage : validez d'abord le dérush (bouton « Valider le dérush » de la page des moments).</p><button type=\"button\">Aller à la page des moments</button>";
    box.querySelector("h1").textContent = current[1];
    box.querySelector("button").addEventListener("click", () => { location.href = "/"; });
    document.body.append(box);
  }
  // Claude Code : pastille « Nécessite Claude Code » sur les parties qui en ont besoin ; sans
  // connexion, elles sont grisées et rendues inutilisables (inert), et la pastille mène à la
  // page de connexion. Même chose pour « Discuter avec Claude » dans le menu.
  function markClaude(connected) {
    for (const zone of document.querySelectorAll("[data-needs-claude]")) {
      let tag = zone.querySelector(":scope > .am-needs");
      if (!tag) {
        tag = document.createElement(connected ? "span" : "a");
        tag.className = "am-needs";
        zone.prepend(tag);
      }
      tag.textContent = connected ? "Nécessite Claude Code" : "Nécessite Claude Code · se connecter";
      if (!connected) { tag.href = "/claude/"; tag.title = "Claude Code n'est pas connecté : cette partie ne fonctionne pas sans lui"; }
      zone.classList.toggle("am-claude-off", !connected);
      for (const child of zone.children) if (child !== tag) child.inert = !connected;
    }
    const chat = document.querySelector('.am-panel a[data-href="/chat/"]');
    if (chat) {
      const b = chat.querySelector("b");
      if (!b.querySelector(".am-cc")) b.append(Object.assign(document.createElement("span"), { className: "am-badge am-cc", textContent: "Claude Code" }));
      chat.classList.toggle("am-off", !connected);
      if (!connected) {
        chat.removeAttribute("href");
        chat.setAttribute("aria-disabled", "true");
        chat.querySelector("small").textContent = "Nécessite Claude Code (pas connecté)";
      }
    }
  }
  // Confirmation dans une fenêtre de la page (au lieu de confirm() du navigateur, qui bloque la
  // page et ne se met pas en forme) : amConfirm(texte, { title, ok, cancel, danger }) → promesse
  // de true (bouton ok) ou false (Annuler, Échap). Avec check: { label, checked } : une case sous le
  // texte, et la promesse donne { checked } (bouton ok) ou false.
  window.amConfirm = (text, o = {}) => new Promise((resolve) => {
    if (!document.querySelector("style[data-am-ask]")) {
      const st = document.createElement("style");
      st.dataset.amAsk = "1";
      st.textContent = css.slice(css.indexOf("  /* Fenêtre de confirmation"), css.indexOf("  .am-bar :focus-visible"));
      document.head.append(st);
    }
    const dlg = document.createElement("dialog");
    dlg.className = "am-ask";
    dlg.innerHTML = '<h2></h2><p></p><div class="am-ask-foot"><button type="button" class="am-ask-no"></button><button type="button" class="am-ask-ok"></button></div>';
    dlg.querySelector("h2").textContent = o.title || "Confirmer";
    dlg.querySelector("p").textContent = text;
    const ok = dlg.querySelector(".am-ask-ok"), no = dlg.querySelector(".am-ask-no");
    ok.textContent = o.ok || "Continuer";
    no.textContent = o.cancel || "Annuler";
    if (o.danger) ok.classList.add("am-danger");
    let box = null;
    if (o.check) {
      const lab = document.createElement("label");
      lab.className = "am-ask-check";
      box = Object.assign(document.createElement("input"), { type: "checkbox", checked: !!o.check.checked });
      lab.append(box, " ", o.check.label);
      dlg.querySelector("p").after(lab);
    }
    let answer = false;
    ok.addEventListener("click", () => { answer = true; dlg.close(); });
    no.addEventListener("click", () => dlg.close());
    dlg.addEventListener("close", () => { dlg.remove(); resolve(answer && box ? { checked: box.checked } : answer); });
    document.body.append(dlg);
    dlg.showModal();
    ok.focus();
  });
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", build);
  else build();
})();
