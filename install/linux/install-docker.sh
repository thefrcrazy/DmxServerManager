#!/bin/sh
set -eu

umask 077

install_dir=${DMX_INSTALL_DIR:-/opt/dmx-server-manager}
image=${DMX_IMAGE:-ghcr.io/thefrcrazy/dmx-server-manager:latest}
legacy_dir=${DMX_LEGACY_DIR:-}
no_start=${DMX_NO_START:-0}

if [ "$(id -u)" -ne 0 ]; then
    echo "Run this installer with sudo/root." >&2
    exit 77
fi

case "$(uname -m)" in
    x86_64|amd64) ;;
    *)
        echo "DmxServerManager Docker supports Linux AMD64 only." >&2
        exit 64
        ;;
esac

case "$install_dir" in
    /*) ;;
    *)
        echo "DMX_INSTALL_DIR must be an absolute path." >&2
        exit 64
        ;;
esac
case "$install_dir" in
    *[!A-Za-z0-9_./-]*|*/../*|*/..)
        echo "DMX_INSTALL_DIR contains unsupported path components." >&2
        exit 64
        ;;
esac

case "$image" in
    ghcr.io/thefrcrazy/dmx-server-manager@sha256:*)
        image_digest=${image#*@sha256:}
        if [ "${#image_digest}" -ne 64 ] || printf '%s\n' "$image_digest" | grep -q '[^0-9a-f]'; then
            echo "DMX_IMAGE contains an invalid sha256 digest." >&2
            exit 64
        fi
        ;;
    ghcr.io/thefrcrazy/dmx-server-manager:*)
        image_tag=${image#*:}
        if ! printf '%s\n' "$image_tag" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'; then
            echo "DMX_IMAGE contains an invalid container tag." >&2
            exit 64
        fi
        ;;
    *)
        echo "DMX_IMAGE must use the official GHCR image by tag or sha256 digest." >&2
        exit 64
        ;;
esac

timezone=${DMX_TIMEZONE:-Etc/UTC}
if ! printf '%s\n' "$timezone" | grep -Eq '^[A-Za-z0-9_+./-]+$'; then
    echo "DMX_TIMEZONE contains unsupported characters." >&2
    exit 64
fi

case "$no_start" in
    0|1) ;;
    *)
        echo "DMX_NO_START must be 0 or 1." >&2
        exit 64
        ;;
esac

for command_name in curl docker openssl; do
    command -v "$command_name" >/dev/null 2>&1 || {
        echo "$command_name is required." >&2
        exit 69
    }
done
docker compose version >/dev/null 2>&1 || {
    echo "Docker Compose v2 is required (docker compose)." >&2
    exit 69
}

if [ -L "$install_dir" ] || { [ -e "$install_dir" ] && [ ! -d "$install_dir" ]; }; then
    echo "Refusing a linked or non-directory install path: $install_dir" >&2
    exit 78
fi

caller_uid=${SUDO_UID:-0}
caller_gid=${SUDO_GID:-0}
case "$caller_uid:$caller_gid" in
    *[!0-9:]*)
        echo "Invalid sudo caller uid/gid." >&2
        exit 64
        ;;
esac
config_dir="$install_dir/config"
data_dir="$install_dir/data"
compose_file="$install_dir/docker-compose.yml"
env_file="$install_dir/.env"
master_key="$config_dir/master.key"
setup_token_file="$config_dir/setup-token"

mkdir -p "$install_dir"
for managed_path in "$config_dir" "$data_dir"; do
    if [ -L "$managed_path" ] || { [ -e "$managed_path" ] && [ ! -d "$managed_path" ]; }; then
        echo "Refusing a linked or non-directory managed path: $managed_path" >&2
        exit 78
    fi
done
mkdir -p "$config_dir" "$data_dir"

for regular_path in \
    "$compose_file" \
    "$env_file" \
    "$config_dir/config.toml" \
    "$master_key" \
    "$setup_token_file"; do
    if [ -L "$regular_path" ] || { [ -e "$regular_path" ] && [ ! -f "$regular_path" ]; }; then
        echo "Refusing a linked or non-regular managed file: $regular_path" >&2
        exit 78
    fi
done

if [ -n "$legacy_dir" ]; then
    case "$legacy_dir" in
        /*) ;;
        *)
            echo "DMX_LEGACY_DIR must be an absolute path." >&2
            exit 64
            ;;
    esac
    case "$legacy_dir" in
        *[!A-Za-z0-9_./-]*|*/../*|*/..)
            echo "DMX_LEGACY_DIR contains unsupported path components." >&2
            exit 64
            ;;
    esac
    if [ -L "$legacy_dir" ] || [ ! -d "$legacy_dir" ]; then
        echo "Invalid legacy install directory: $legacy_dir" >&2
        exit 78
    fi
    legacy_key="$legacy_dir/secrets/master.key"
    if [ -L "$legacy_key" ] || [ ! -f "$legacy_key" ]; then
        echo "Missing regular legacy master key: $legacy_key" >&2
        exit 78
    fi
    if [ "$(wc -c < "$legacy_key" | tr -d ' ')" -ne 32 ]; then
        echo "The legacy master key must contain exactly 32 bytes." >&2
        exit 78
    fi
    if [ -f "$legacy_dir/docker-compose.yml" ]; then
        echo "Stopping the legacy Compose stack before copying SQLite data..."
        docker compose --project-directory "$legacy_dir" -f "$legacy_dir/docker-compose.yml" down
    fi
    if [ ! -e "$master_key" ]; then
        cp "$legacy_key" "$master_key"
        echo "Copied the legacy master key into config/master.key."
    fi
fi

if [ -L "$master_key" ] || { [ -e "$master_key" ] && [ ! -f "$master_key" ]; }; then
    echo "Refusing a linked or non-regular master key: $master_key" >&2
    exit 78
fi
if [ ! -e "$master_key" ]; then
    openssl rand 32 > "$master_key"
    echo "Created config/master.key. Back it up separately from data/."
fi
if [ "$(wc -c < "$master_key" | tr -d ' ')" -ne 32 ]; then
    echo "The master key must contain exactly 32 bytes." >&2
    exit 78
fi

if [ ! -e "$setup_token_file" ]; then
    openssl rand -base64 32 | tr -d '\n' > "$setup_token_file"
fi

if [ ! -e "$config_dir/config.toml" ]; then
    cat > "$config_dir/config.toml" <<'EOF_CONFIG'
bind = "127.0.0.1:5500"
data_dir = "/data"
database_url = "sqlite:///data/dmx-server-manager.sqlite?mode=rwc"
master_key_file = "/config/master.key"
steamcmd_path = "/usr/games/steamcmd"
static_dir = "/opt/dmx-server-manager/static"
reverse_proxy = false
trusted_proxies = []
import_roots = []
log = "info"
deployment_mode = "docker"
session_ttl_hours = 24

# Avec un reverse proxy HTTPS externe, utilisez une adresse d'écoute joignable,
# passez reverse_proxy à true et déclarez uniquement son IP exacte dans
# trusted_proxies. N'exposez jamais le port 5500 en HTTP public.
EOF_CONFIG
fi

if [ ! -e "$compose_file" ]; then
    cat > "$compose_file" <<'EOF_COMPOSE'
name: dmx-server-manager

services:
  panel:
    image: "${DMX_IMAGE:-ghcr.io/thefrcrazy/dmx-server-manager:latest}"
    platform: linux/amd64
    restart: unless-stopped
    network_mode: host
    user: "10001:10001"
    read_only: true
    cap_drop:
      - ALL
    security_opt:
      - no-new-privileges:true
    stop_grace_period: 2m
    pids_limit: 4096
    environment:
      TZ: ${DMX_TIMEZONE:-Etc/UTC}
      DMX_CONFIG_FILE: /config/config.toml
      DMX_SETUP_TOKEN: ${DMX_SETUP_TOKEN:-}
    volumes:
      - ./config:/config:ro
      - ./data:/data
    tmpfs:
      - /tmp:size=256m,mode=1777
      - /run:size=16m,mode=0755
    healthcheck:
      test: ["CMD", "curl", "--fail", "--silent", "--show-error", "http://127.0.0.1:5500/api/v1/health"]
      interval: 30s
      timeout: 5s
      retries: 5
      start_period: 30s
EOF_COMPOSE
fi

if [ ! -e "$env_file" ]; then
    {
        printf 'DMX_IMAGE=%s\n' "$image"
        printf 'DMX_TIMEZONE=%s\n' "$timezone"
        printf 'DMX_SETUP_TOKEN=%s\n' "$(cat "$setup_token_file")"
    } > "$env_file"
elif [ "${DMX_IMAGE+x}" = x ]; then
    temporary_env="$env_file.tmp.$$"
    if grep -q '^DMX_IMAGE=' "$env_file"; then
        sed "s|^DMX_IMAGE=.*|DMX_IMAGE=$image|" "$env_file" > "$temporary_env"
        mv "$temporary_env" "$env_file"
    else
        printf 'DMX_IMAGE=%s\n' "$image" >> "$env_file"
    fi
fi

chown "$caller_uid:$caller_gid" "$install_dir" "$compose_file" "$env_file" "$setup_token_file"
chown root:10001 "$config_dir" "$config_dir/config.toml"
chown 10001:10001 "$master_key" "$data_dir"
chmod 0750 "$install_dir" "$config_dir"
chmod 0700 "$data_dir"
chmod 0640 "$config_dir/config.toml"
chmod 0600 "$env_file" "$setup_token_file"
chmod 0400 "$master_key"

if [ "$no_start" -eq 1 ]; then
    echo "Generated $compose_file without starting Docker."
    exit 0
fi

cd "$install_dir"
docker compose pull panel

effective_image=$(sed -n 's/^DMX_IMAGE=//p' "$env_file" | tail -n 1)
if [ -z "$effective_image" ]; then
    effective_image=$image
fi

legacy_volume=dmx-server-manager-data
if ! find "$data_dir" -mindepth 1 -print -quit | grep -q . \
    && docker volume inspect "$legacy_volume" >/dev/null 2>&1; then
    if docker run --rm --entrypoint sh \
        --volume "$legacy_volume:/source:ro" \
        "$effective_image" -c 'find /source -mindepth 1 -print -quit | grep -q .' >/dev/null 2>&1; then
        if docker ps --quiet --filter "volume=$legacy_volume" | grep -q .; then
            echo "The legacy volume is still used by a running container." >&2
            echo "Stop its Compose stack before retrying the migration." >&2
            exit 78
        fi
        if [ -z "$legacy_dir" ]; then
            echo "A non-empty legacy Docker volume was found: $legacy_volume" >&2
            echo "Rerun with DMX_LEGACY_DIR=/absolute/path/to/old/install/linux." >&2
            exit 78
        fi
        if find "$data_dir" -mindepth 1 -print -quit | grep -q .; then
            echo "Refusing to overwrite the non-empty data directory: $data_dir" >&2
            exit 78
        fi
        echo "Copying the legacy named volume into data/ without deleting the source..."
        docker run --rm --user 0:0 --entrypoint sh \
            --volume "$legacy_volume:/source:ro" \
            --volume "$data_dir:/target" \
            "$effective_image" -c 'cp -a /source/. /target/ && chown -R 10001:10001 /target'
    fi
fi

docker compose up -d --force-recreate panel
for attempt in $(seq 1 45); do
    : "$attempt"
    if curl --fail --silent http://127.0.0.1:5500/api/v1/health >/dev/null; then
        echo "DmxServerManager is running."
        echo "Compose: $compose_file"
        echo "Configuration: $config_dir/config.toml"
        echo "Initial setup token: $setup_token_file"
        echo "Update: cd $install_dir && docker compose pull panel && docker compose up -d --force-recreate panel"
        exit 0
    fi
    if ! docker compose ps --status running --services | grep -qx panel; then
        docker compose logs panel >&2 || true
        exit 1
    fi
    sleep 2
done

docker compose logs panel >&2 || true
echo "The panel did not become healthy in time." >&2
exit 1
