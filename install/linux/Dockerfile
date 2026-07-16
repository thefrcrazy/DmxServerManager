FROM oven/bun:1.3.14-debian@sha256:9dba1a1b43ce28c9d7931bfc4eb00feb63b0114720a0277a8f939ae4dfc9db6f AS frontend-builder

WORKDIR /src/frontend
COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile
COPY frontend/ ./
RUN bun run build

FROM rust:1.97.0-bookworm@sha256:8fa55b2f3ddf97471ab6a767bfa3f37e6bad0986ba823e75fea57e2a2a5c3073 AS backend-builder

WORKDIR /src/backend
COPY backend/ ./
COPY --from=frontend-builder /src/frontend/dist ./static
RUN --mount=type=cache,id=dmx-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=dmx-cargo-target-amd64,target=/src/backend/target \
    cargo build --locked --release --jobs 1 --bin dmx-server-manager \
    && install -m 0555 target/release/dmx-server-manager /tmp/dmx-server-manager

# Java 25 is required by the Hytale profile and by recent Minecraft profiles.
# SteamCMD is installed from Ubuntu's native multiverse repository as required by
# the anonymous-Steam-only support contract. No game binary is bundled.
FROM eclipse-temurin:25.0.3_9-jdk-noble@sha256:3eb81ed94d8c1a34422f19f8188548bdf02cae69c91d0328afdbb7abed90f617

LABEL org.opencontainers.image.title="DmxServerManager" \
      org.opencontainers.image.description="Single-host game server manager" \
      org.opencontainers.image.source="https://github.com/thefrcrazy/DmxServerManager" \
      org.opencontainers.image.licenses="MIT"

ARG DEBIAN_FRONTEND=noninteractive
RUN dpkg --add-architecture i386 \
    && sed -i '/^Components:/ s/$/ multiverse/' /etc/apt/sources.list.d/ubuntu.sources \
    && sed -E -i \
        -e 's|https?://archive.ubuntu.com/ubuntu/|https://snapshot.ubuntu.com/ubuntu/20260713T000000Z/|' \
        -e 's|https?://security.ubuntu.com/ubuntu/|https://snapshot.ubuntu.com/ubuntu/20260713T000000Z/|' \
        /etc/apt/sources.list.d/ubuntu.sources \
    && ! grep -Eq 'archive\.ubuntu\.com|security\.ubuntu\.com' /etc/apt/sources.list.d/ubuntu.sources \
    && grep -Fq 'https://snapshot.ubuntu.com/ubuntu/20260713T000000Z/' /etc/apt/sources.list.d/ubuntu.sources \
    && echo 'steam steam/question select I AGREE' | debconf-set-selections \
    && echo 'steam steam/license note' | debconf-set-selections \
    && apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        curl \
        git \
        libatomic1 \
        lib32gcc-s1 \
        lib32stdc++6 \
        libpulse0 \
        steamcmd \
        tini \
        unzip \
        xdg-user-dirs \
        xz-utils \
        zip \
    && ln -s /usr/games/steamcmd /usr/local/bin/steamcmd \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/*

RUN groupadd --gid 10001 dmx-server-manager \
    && useradd --uid 10001 --gid 10001 --create-home \
        --home-dir /home/dmx-server-manager --shell /usr/sbin/nologin \
        dmx-server-manager \
    && install -d -o 10001 -g 10001 -m 0700 \
        /config /data /data/home /data/instances /data/backups /data/toolchains \
    && install -d -o 10001 -g 10001 -m 0755 /opt/dmx-server-manager

WORKDIR /opt/dmx-server-manager
COPY --from=backend-builder --chown=10001:10001 /tmp/dmx-server-manager ./dmx-server-manager
COPY --from=frontend-builder --chown=10001:10001 /src/frontend/dist ./static
COPY --chown=10001:10001 install/linux/entrypoint.sh ./entrypoint.sh
RUN chmod 0555 ./dmx-server-manager ./entrypoint.sh

ENV HOME=/data/home \
    DMX_CONFIG_FILE=/config/config.toml \
    PATH=/data/toolchains/bin:/opt/java/openjdk/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

VOLUME ["/config", "/data"]
EXPOSE 5500

USER 10001:10001
ENTRYPOINT ["/usr/bin/tini", "--", "/opt/dmx-server-manager/entrypoint.sh"]
CMD ["/opt/dmx-server-manager/dmx-server-manager"]
