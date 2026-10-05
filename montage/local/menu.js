// Bandeau commun aux pages de l'interface locale : nom d'Auto-montage, page en cours, et un
// menu déroulant vers les autres pages (moments, rushes, téléchargements, Claude, mises à
// jour, journal). Ajouté par scripts/local_server.py à toutes les pages ; les pages publiées
// sur claude.ai ne l'ont pas. Couleurs : celles de la page (variables --bg, --accent…).
(() => {
  const PAGES = [
    ["/", "Page des moments", "Choisir les moments, effets, découpe, génération"],
    ["/rush/", "Rushes et montages", "Nouveau rush, consignes, montages enregistrés"],
    ["/downloads/", "Téléchargements", "Version de travail en 720p, 1080p, 4K"],
    ["/chat/", "Discuter avec Claude", "Changer le montage en lui écrivant"],
    ["/claude/", "Connexion à Claude", "Relier votre compte claude.ai"],
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
        <button type="button" class="am-toggle" aria-expanded="false" aria-controls="am-panel">
          <span class="am-dot" hidden></span>Menu<span class="am-chev" aria-hidden="true">▾</span></button>
      </div>
      <div class="am-panel" id="am-panel" role="navigation" aria-label="Pages d'Auto-montage" hidden><ul></ul></div>`;
    const toggle = bar.querySelector(".am-toggle");
    const panel = bar.querySelector(".am-panel");
    bar.querySelector(".am-here").textContent = current ? current[1] : "";
    for (const [href, label, hint] of PAGES) {
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
    const open = (yes) => { panel.hidden = !yes; toggle.setAttribute("aria-expanded", String(yes)); };
    toggle.addEventListener("click", () => open(panel.hidden));
    document.addEventListener("click", (e) => { if (!bar.contains(e.target)) open(false); });
    document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !panel.hidden) { open(false); toggle.focus(); } });
    document.body.prepend(bar);
    // Bandeau de bord à bord, malgré la marge intérieure de la page.
    const pad = getComputedStyle(document.body);
    bar.style.marginLeft = `-${pad.paddingLeft}`;
    bar.style.marginRight = `-${pad.paddingRight}`;

    // Nouvelle version disponible : pastille sur le menu et sur « Mises à jour ».
    fetch("/api/status").then((r) => r.json()).then((st) => {
      markClaude(!!st.claude);
      // Premier lancement : le tutoriel d'abord (une seule fois, il le note en s'ouvrant).
      if (st.tutorialSeen === false && here !== "/tutoriel/") { location.href = "/tutoriel/?bienvenue=1"; return; }
      if (!st.update || !st.update.newer) return;
      bar.querySelector(".am-dot").hidden = false;
      const b = panel.querySelector('a[data-href="/updates/"] b');
      const badge = Object.assign(document.createElement("span"), { className: "am-badge", textContent: `${st.update.latest} disponible` });
      b.append(badge);
    }).catch(() => {});
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
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", build);
  else build();
})();
