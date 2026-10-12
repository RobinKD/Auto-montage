# Auto-montage : Claude Code + HyperFrames + Whisper + interface web locale (port 8080).
# Le dépôt est monté dans /app (rushes, rendus et fichiers de travail restent sur la machine) ;
# le programme Auto-montage (Rust, montage/rust/) est compilé au démarrage depuis le dépôt monté,
# puis les dépendances lourdes (ffmpeg, paquets Node, Whisper et ses modèles, police) sont
# installées au premier démarrage par « auto-montage setup ».
FROM docker.io/library/node:22-bookworm-slim AS node
FROM docker.io/library/rust:1-slim-bookworm AS rust

FROM docker.io/library/ubuntu:22.04
# Repère des images d'Auto-montage : les anciennes versions (sans nom après une reconstruction)
# sont effacées par les lanceurs (docker/lib.sh am_up, windows/open.ps1).
LABEL auto-montage=1
ENV DEBIAN_FRONTEND=noninteractive

# Outils de compilation (Auto-montage, whisper.cpp), et bibliothèques du Chrome de rendu de HyperFrames.
RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 build-essential cmake git curl ca-certificates unzip xz-utils \
      libnss3 libdbus-1-3 libatk1.0-0 libatk-bridge2.0-0 libcups2 libdrm2 libgbm1 libasound2 \
      libxkbcommon0 libxcomposite1 libxdamage1 libxfixes3 libxrandr2 libpango-1.0-0 libcairo2 \
      libglib2.0-0 fonts-liberation \
      fontconfig fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*

# Node.js 22 (HyperFrames, Claude Code), repris de l'image officielle.
COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && node --version && npm --version

# Rust (compilation d'Auto-montage), repris de l'image officielle. Programme compilé et cache des
# paquets Rust dans le volume /opt/auto-montage (compose.yaml) : la compilation suivante ne refait
# que ce qui a changé (mise à jour d'Auto-montage).
COPY --from=rust /usr/local/cargo /usr/local/cargo
COPY --from=rust /usr/local/rustup /usr/local/rustup
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/opt/auto-montage/cargo \
    CARGO_TARGET_DIR=/opt/auto-montage/target \
    PATH=/usr/local/cargo/bin:$PATH
RUN cargo --version && ln -s /opt/auto-montage/target/release/auto-montage /usr/local/bin/auto-montage

# Claude Code (mis à jour en reconstruisant l'image : docker compose build --pull).
RUN npm install -g @anthropic-ai/claude-code
# Les plugins Claude Code se clonent depuis GitHub : en HTTPS (pas de client ni de clé SSH ici).
RUN git config --system url."https://github.com/".insteadOf "git@github.com:" \
    && git config --system --add url."https://github.com/".insteadOf "ssh://git@github.com/"
ENV DISABLE_AUTOUPDATER=1

# Dossier personnel inscriptible quel que soit l'utilisateur (compose : AM_UID/AM_GID),
# gardé dans un volume : connexion Claude Code, plugins, caches npm.
ENV HOME=/home/app
# Whisper et ses modèles : dans le volume /opt/whisper (compose.yaml), hors du dépôt.
ENV WHISPER_DIR=/opt/whisper/whisper.cpp
# Les lanceurs savent qu'ils tournent ici (montage/am).
ENV AM_IN_CONTAINER=1
RUN mkdir -p /home/app && chmod 1777 /home/app

COPY docker/entrypoint.sh /usr/local/bin/auto-montage-entrypoint
RUN sed -i 's/\r$//' /usr/local/bin/auto-montage-entrypoint && chmod +x /usr/local/bin/auto-montage-entrypoint
WORKDIR /app
EXPOSE 8080
ENTRYPOINT ["auto-montage-entrypoint"]
CMD ["serve"]
