# Auto-montage : Claude Code + HyperFrames + Whisper + interface web locale (port 8080).
# Le dépôt est monté dans /app (rushes, rendus et fichiers de travail restent sur la machine) ;
# les dépendances lourdes du projet (paquets Node, Whisper et ses modèles, police) sont
# installées au premier démarrage par montage/scripts/setup.sh, dans le dépôt monté.
FROM docker.io/library/node:22-bookworm-slim AS node

FROM docker.io/library/ubuntu:22.04
# Repère des images d'Auto-montage : les anciennes versions (sans nom après une reconstruction)
# sont effacées par les lanceurs (docker/lib.sh am_up, windows/open.ps1).
LABEL auto-montage=1
ENV DEBIAN_FRONTEND=noninteractive

# Python, outils de compilation (whisper.cpp), et bibliothèques du Chrome de rendu de HyperFrames.
RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 python3-pip build-essential cmake git curl ca-certificates unzip xz-utils \
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

# Paquets Python du projet (ffmpeg complet, numpy, OpenCV), installés pour tout le système.
ENV PIP_BREAK_SYSTEM_PACKAGES=1
COPY montage/scripts/requirements.txt /tmp/requirements.txt
RUN pip3 install --no-cache-dir -r /tmp/requirements.txt

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
# Les scripts du projet savent qu'ils tournent ici (scripts/in_container.sh).
ENV AM_IN_CONTAINER=1
RUN mkdir -p /home/app && chmod 1777 /home/app

COPY docker/entrypoint.sh /usr/local/bin/auto-montage
RUN sed -i 's/\r$//' /usr/local/bin/auto-montage && chmod +x /usr/local/bin/auto-montage
WORKDIR /app
EXPOSE 8080
ENTRYPOINT ["auto-montage"]
CMD ["serve"]
