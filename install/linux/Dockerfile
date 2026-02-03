# ==========================================
# Stage 1: Build Frontend
# ==========================================
FROM oven/bun:1 AS frontend-builder

WORKDIR /app/frontend
COPY frontend/package.json frontend/bun.lock* ./
RUN bun install --frozen-lockfile

COPY frontend/ ./
RUN bun run build

# ==========================================
# Stage 2: Build Backend
# ==========================================
FROM rust:latest AS backend-builder

WORKDIR /app/backend
COPY backend/Cargo.toml backend/Cargo.lock* ./
COPY backend/src ./src

RUN nice -n 19 cargo build --release --jobs 2

# ==========================================
# Stage 3: Runtime
# ==========================================
FROM debian:testing-slim

# Install runtime dependencies and Java 25
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    wget \
    gnupg \
    && mkdir -p /etc/apt/keyrings \
    && wget -O - https://packages.adoptium.net/artifactory/api/gpg/key/public | tee /etc/apt/keyrings/adoptium.asc \
    && echo "deb [signed-by=/etc/apt/keyrings/adoptium.asc] https://packages.adoptium.net/artifactory/deb bookworm main" | tee /etc/apt/sources.list.d/adoptium.list \
    && apt-get update \
    && apt-get install -y temurin-25-jdk gosu curl unzip dbus \
    && rm -rf /var/lib/apt/lists/* \
    && dbus-uuidgen --ensure=/etc/machine-id

# Create non-root user with explicit UID 1000 to match host user
RUN useradd -u 1000 -m -d /home/draveur -s /bin/bash draveur

WORKDIR /app

# Copy entrypoint script
COPY install/linux/entrypoint.sh ./entrypoint.sh
RUN chmod +x ./entrypoint.sh

# Copy backend binary
COPY --from=backend-builder /app/backend/target/release/draveur ./draveur

# Copy frontend static files
COPY --from=frontend-builder /app/frontend/dist ./static

# Create data directories
RUN mkdir -p /data /servers /backups

# Environment
ENV HOST=0.0.0.0
ENV PORT=5500
ENV IS_DOCKER=true
ENV DATABASE_URL=sqlite:/data/database.db?mode=rwc
ENV SERVERS_DIR=/servers
ENV BACKUPS_DIR=/backups
ENV UPLOADS_DIR=/data/uploads
ENV RUST_LOG=info

EXPOSE 5500

ENTRYPOINT ["/app/entrypoint.sh"]
CMD ["./draveur"]
