#!/bin/bash
# Installe tout ce qu'il faut pour préparer et rendre les montages dans une
# session Claude Code sur le web. Idempotent : chaque étape saute ce qui existe déjà.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

# Plugin Claude Code (skills HyperFrames). Aussi déclaré dans .claude/settings.json, qui
# l'active dès le démarrage de la session.
claude plugin marketplace add heygen-com/hyperframes || echo "Marketplace hyperframes indisponible"
claude plugin install hyperframes@hyperframes || echo "Plugin hyperframes non installé"

cd "$CLAUDE_PROJECT_DIR/montage"

# Domaines réseau : signale ceux à autoriser, sans bloquer le démarrage.
./scripts/check_network.sh || echo "Certains domaines sont bloqués : voir env/allowed-domains.txt"

# Paquets Node et Python, navigateur de rendu, Whisper et ses modèles, police Oliver,
# modèle de détection du visage : script commun avec le conteneur Docker.
./scripts/setup.sh
