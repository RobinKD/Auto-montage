// Remplace, en local, ce que claude.ai fournit aux pages (window.claude.use) :
//   db        documents JSON enregistrés par scripts/local_server.py (work/local_db/)
//   downloads enregistrement direct d'un fichier par le navigateur
//   mcp       présent pour activer le bouton « Générer » (la page appelle /api/regen)
// Même interface que sur claude.ai pour ce que les pages utilisent.
(() => {
  window.AUTO_MONTAGE_LOCAL = true;
  const POLL_MS = 1500;
  const api = (path, init) => fetch(`/api/db/${path}`, init).then((r) => {
    if (!r.ok) throw Object.assign(new Error(`db ${r.status}`), { code: "unavailable" });
    return r.json();
  });

  // Abonnements : relit la collection régulièrement et prévient quand elle change.
  const watchers = new Set();
  async function poll(w) {
    try {
      const { docs } = await api(w.collection);
      const key = JSON.stringify(docs);
      if (key !== w.last) { w.last = key; w.emit(docs); }
    } catch (e) { if (w.onError) w.onError(e); }
  }
  setInterval(() => watchers.forEach(poll), POLL_MS);
  function watch(collection, emit, onError) {
    const w = { collection, emit, onError, last: null };
    watchers.add(w);
    poll(w);
    return () => watchers.delete(w);
  }
  const refresh = (collection) => watchers.forEach((w) => { if (w.collection === collection) poll(w); });

  const docSnap = (id, data) => ({ id, exists: data !== undefined, data: () => data, metadata: { hasPendingWrites: false } });
  const db = {
    doc(path) {
      const [collection, id] = path.split("/");
      return {
        async get() { const { docs } = await api(collection); const d = docs.find((x) => x.id === id); return docSnap(id, d && d.data); },
        async set(data) {
          await api(`${collection}/${encodeURIComponent(id)}`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify(data) });
          refresh(collection);
        },
        onSnapshot(cb, onError) {
          return watch(collection, (docs) => { const d = docs.find((x) => x.id === id); cb(docSnap(id, d && d.data)); }, onError);
        },
      };
    },
    collection(collection) {
      return {
        onSnapshot(cb, onError) {
          return watch(collection, (docs) => cb({ docs: docs.map((d) => docSnap(d.id, d.data)) }), onError);
        },
      };
    },
  };

  const downloads = {
    async save({ filename, data }) {
      const blob = data instanceof Blob ? data : new Blob([data]);
      const url = URL.createObjectURL(blob);
      const a = Object.assign(document.createElement("a"), { href: url, download: filename });
      document.body.append(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 60000);
      return { status: "saved" };
    },
  };

  const mcp = {
    async callTool() { throw Object.assign(new Error("Pas de connecteur en local"), { code: "server_not_connected" }); },
  };

  const caps = { db, downloads, mcp };
  window.claude = { use: async (name) => caps[name] ?? null };
})();
