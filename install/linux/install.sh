#!/bin/sh
set -eu

PRODUCT=dmx-server-manager
SERVICE_USER=${DMX_SERVICE_USER:-dmx-server-manager}
SERVICE_GROUP=${DMX_SERVICE_GROUP:-dmx-server-manager}
VERSION=${DMX_VERSION:-1.0.19}
EXPECTED_ARCHIVE_SHA256=${DMX_EXPECTED_ARCHIVE_SHA256:-}
ARCHIVE_FILE=${DMX_ARCHIVE_FILE:-}
INSTALL_GAME_TOOLCHAINS=${DMX_INSTALL_GAME_TOOLCHAINS:-1}
STEAMCMD_PATH=${DMX_STEAMCMD_PATH:-}
DATA_DIR=${DMX_DATA_DIR:-/var/lib/dmx-server-manager}
CONFIG_DIR=${DMX_CONFIG_DIR:-/etc/dmx-server-manager}
INSTALL_ROOT=${DMX_INSTALL_ROOT:-/usr/lib/dmx-server-manager}
REPOSITORY=${DMX_REPOSITORY:-thefrcrazy/DmxServerManager}
BASE_URL=${DMX_RELEASE_BASE_URL:-https://github.com/$REPOSITORY/releases/download/v$VERSION}
ASSET="$PRODUCT-v$VERSION-x86_64-unknown-linux-gnu.tar.gz"
SERVICE_FILE=/etc/systemd/system/dmx-server-manager.service
CLI_LINK=/usr/local/bin/dmx-server-manager
HEALTH_URL=${DMX_HEALTH_URL:-http://127.0.0.1:5500/api/v1/health}

tmp_dir=''
staging_dir=''
install_lock=''
transaction_started=false
transaction_succeeded=false
previous_release=''
service_was_active=false
service_was_enabled=false
service_unit_existed=false
cli_link_created=false

die() {
    echo "Error: $*" >&2
    exit 1
}

warn() {
    echo "Warning: $*" >&2
}

validate_account_name() {
    label=$1
    value=$2
    case "$value" in
        ''|[!A-Za-z_]*|*[!A-Za-z0-9_-]*) die "$label contains unsupported characters" ;;
    esac
}

validate_absolute_path() {
    label=$1
    value=$2
    case "$value" in
        /*) ;;
        *) die "$label must be an absolute path" ;;
    esac
    case "$value" in
        /|*[!A-Za-z0-9_./+-]*|*/../*|*/..) die "$label contains an unsupported path" ;;
    esac
}

validate_steamcmd_path() {
    candidate=$1
    case "$candidate" in
        /*) ;;
        *) die "DMX_STEAMCMD_PATH must be an absolute path" ;;
    esac
    case "$candidate" in
        *[!A-Za-z0-9_./+-]*) die "DMX_STEAMCMD_PATH contains unsupported characters" ;;
    esac
    [ -f "$candidate" ] || die "SteamCMD is not a regular file: $candidate"
    [ -x "$candidate" ] || die "SteamCMD is not executable: $candidate"
    STEAMCMD_PATH=$candidate
}

find_packaged_steamcmd() {
    for candidate in /usr/games/steamcmd /usr/bin/steamcmd /usr/local/bin/steamcmd; do
        if [ -f "$candidate" ] && [ -x "$candidate" ]; then
            STEAMCMD_PATH=$candidate
            return 0
        fi
    done
    return 1
}

install_game_toolchains() {
    if [ -n "$STEAMCMD_PATH" ]; then
        validate_steamcmd_path "$STEAMCMD_PATH"
    else
        find_packaged_steamcmd || true
    fi

    need_git=false
    need_steamcmd=false
    command -v git >/dev/null 2>&1 || need_git=true
    [ -n "$STEAMCMD_PATH" ] || need_steamcmd=true
    if [ "$need_git" = false ] && [ "$need_steamcmd" = false ]; then
        return
    fi
    if [ "$INSTALL_GAME_TOOLCHAINS" != 1 ]; then
        warn "automatic Git/SteamCMD package installation was disabled"
    elif command -v apt-get >/dev/null 2>&1 && command -v dpkg >/dev/null 2>&1; then
        if [ "$need_steamcmd" = true ]; then
            if ! dpkg --print-foreign-architectures | grep -qx i386; then
                dpkg --add-architecture i386
            fi
            if command -v debconf-set-selections >/dev/null 2>&1; then
                printf '%s\n' \
                    'steam steam/question select I AGREE' \
                    'steam steam/license note' \
                    | debconf-set-selections
            fi
        fi
        echo "Installing native game toolchains from the configured APT repositories..."
        if ! apt-get update; then
            packages_installed=false
        elif [ "$need_git" = true ] && [ "$need_steamcmd" = true ]; then
            if DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends git steamcmd; then
                packages_installed=true
            else
                packages_installed=false
            fi
        elif [ "$need_git" = true ]; then
            if DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends git; then
                packages_installed=true
            else
                packages_installed=false
            fi
        else
            if DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends steamcmd; then
                packages_installed=true
            else
                packages_installed=false
            fi
        fi
        if [ "$packages_installed" != true ]; then
            warn "APT could not install every optional game toolchain; configure the repository component that provides steamcmd and retry"
        fi
    else
        warn "no supported native package manager was detected; install Git and SteamCMD before using Spigot or Steam profiles"
    fi

    if [ -z "$STEAMCMD_PATH" ]; then
        find_packaged_steamcmd || true
    fi
    if [ -z "$STEAMCMD_PATH" ]; then
        # Keep the default deterministic. Steam jobs fail with an explicit
        # diagnostic until the administrator installs the native package.
        STEAMCMD_PATH=/usr/games/steamcmd
        warn "SteamCMD is unavailable; install the OS-native steamcmd package before using Valheim, Palworld or custom Steam profiles"
    fi
    if ! command -v git >/dev/null 2>&1; then
        warn "Git is unavailable; the Spigot BuildTools profile remains disabled until Git is installed"
    fi
}

wait_for_health() {
    attempts=0
    while [ "$attempts" -lt 45 ]; do
        if curl --fail --silent --show-error --max-time 3 "$HEALTH_URL" >/dev/null 2>&1; then
            return 0
        fi
        if ! systemctl is-active --quiet dmx-server-manager.service 2>/dev/null; then
            return 1
        fi
        attempts=$((attempts + 1))
        sleep 2
    done
    return 1
}

restore_previous_installation() {
    set +e
    echo "Installation failed; restoring the previous service state." >&2
    systemctl stop dmx-server-manager.service >/dev/null 2>&1

    if [ -n "$previous_release" ]; then
        rm -f "$INSTALL_ROOT/current.rollback"
        ln -s "$previous_release" "$INSTALL_ROOT/current.rollback"
        mv -Tf "$INSTALL_ROOT/current.rollback" "$INSTALL_ROOT/current"
    else
        rm -f "$INSTALL_ROOT/current"
    fi

    if [ "$service_unit_existed" = true ]; then
        install -o root -g root -m 0644 "$tmp_dir/service.backup" "$SERVICE_FILE"
    else
        rm -f "$SERVICE_FILE"
    fi
    if [ "$cli_link_created" = true ]; then
        rm -f "$CLI_LINK"
    fi

    systemctl daemon-reload >/dev/null 2>&1
    if [ "$service_was_enabled" != true ]; then
        systemctl disable dmx-server-manager.service >/dev/null 2>&1
    fi
    if [ "$service_was_active" = true ]; then
        systemctl start dmx-server-manager.service >/dev/null 2>&1
    fi
    set -e
}

cleanup() {
    status=$?
    trap - EXIT
    if [ "$transaction_started" = true ] && [ "$transaction_succeeded" != true ]; then
        restore_previous_installation
    fi
    if [ -n "$staging_dir" ] && [ -d "$staging_dir" ]; then
        rm -rf "$staging_dir"
    fi
    if [ -n "$tmp_dir" ] && [ -d "$tmp_dir" ]; then
        rm -rf "$tmp_dir"
    fi
    if [ -n "$install_lock" ] && [ -d "$install_lock" ]; then
        rmdir "$install_lock" 2>/dev/null || true
    fi
    exit "$status"
}

trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

[ "$(id -u)" -eq 0 ] || die "run this installer as root"

case "$(uname -m)" in
    x86_64|amd64) ;;
    *) die "DmxServerManager 1.0 supports Linux AMD64 only" ;;
esac

validate_account_name DMX_SERVICE_USER "$SERVICE_USER"
validate_account_name DMX_SERVICE_GROUP "$SERVICE_GROUP"
validate_absolute_path DMX_DATA_DIR "$DATA_DIR"
validate_absolute_path DMX_CONFIG_DIR "$CONFIG_DIR"
validate_absolute_path DMX_INSTALL_ROOT "$INSTALL_ROOT"

case "$VERSION" in
    ''|*[!0-9A-Za-z.+-]*) die "invalid DMX_VERSION: $VERSION" ;;
esac
case "$INSTALL_GAME_TOOLCHAINS" in
    0|1) ;;
    *) die "DMX_INSTALL_GAME_TOOLCHAINS must be 0 or 1" ;;
esac
[ -n "$EXPECTED_ARCHIVE_SHA256" ] \
    || die "DMX_EXPECTED_ARCHIVE_SHA256 is required; obtain it from the verified signed release checksum"
case "$EXPECTED_ARCHIVE_SHA256" in
    *[!0-9A-Fa-f]*) die "DMX_EXPECTED_ARCHIVE_SHA256 must be an exact SHA-256 digest" ;;
esac
[ "${#EXPECTED_ARCHIVE_SHA256}" -eq 64 ] \
    || die "DMX_EXPECTED_ARCHIVE_SHA256 must be an exact SHA-256 digest"
EXPECTED_ARCHIVE_SHA256=$(printf '%s' "$EXPECTED_ARCHIVE_SHA256" | tr 'A-F' 'a-f')

if [ -n "$ARCHIVE_FILE" ]; then
    case "$ARCHIVE_FILE" in
        /*) ;;
        *) die "DMX_ARCHIVE_FILE must be an absolute path" ;;
    esac
    [ -f "$ARCHIVE_FILE" ] || die "DMX_ARCHIVE_FILE is not a regular file: $ARCHIVE_FILE"
    [ -r "$ARCHIVE_FILE" ] || die "DMX_ARCHIVE_FILE is not readable: $ARCHIVE_FILE"
fi

for command_name in curl sha256sum tar systemctl openssl find awk grep getconf getent groupadd useradd id realpath wc; do
    command -v "$command_name" >/dev/null 2>&1 || die "missing required command: $command_name"
done

DATA_DIR=$(realpath -m -- "$DATA_DIR")
CONFIG_DIR=$(realpath -m -- "$CONFIG_DIR")
INSTALL_ROOT=$(realpath -m -- "$INSTALL_ROOT")
for managed_path in "$DATA_DIR" "$CONFIG_DIR" "$INSTALL_ROOT"; do
    [ "$managed_path" != / ] || die "managed paths must not be the filesystem root"
done
paths_overlap() {
    first=$1
    second=$2
    [ "$first" = "$second" ] && return 0
    case "$first/" in "$second/"*) return 0 ;; esac
    case "$second/" in "$first/"*) return 0 ;; esac
    return 1
}
if paths_overlap "$DATA_DIR" "$CONFIG_DIR" \
    || paths_overlap "$DATA_DIR" "$INSTALL_ROOT" \
    || paths_overlap "$CONFIG_DIR" "$INSTALL_ROOT"; then
    die "DMX_DATA_DIR, DMX_CONFIG_DIR and DMX_INSTALL_ROOT must be disjoint paths"
fi

if [ -e "$SERVICE_FILE" ] || [ -L "$SERVICE_FILE" ]; then
    [ -f "$SERVICE_FILE" ] && [ ! -L "$SERVICE_FILE" ] \
        || die "refusing to replace a non-regular systemd unit: $SERVICE_FILE"
    grep -Fqx '# Managed by the DmxServerManager installer.' "$SERVICE_FILE" \
        || die "refusing to replace an unmanaged systemd unit: $SERVICE_FILE"
else
    unit_load_state=$(systemctl show dmx-server-manager.service --property=LoadState --value 2>/dev/null || true)
    case "$unit_load_state" in
        ''|not-found) ;;
        *) die "refusing to shadow an existing systemd unit outside $SERVICE_FILE" ;;
    esac
fi

libc_description=$(getconf GNU_LIBC_VERSION 2>/dev/null) \
    || die "the native archive requires glibc 2.39 or newer; use the Docker image on another libc"
case "$libc_description" in
    'glibc '*) glibc_version=${libc_description#glibc } ;;
    *) die "the native archive requires glibc 2.39 or newer; detected: $libc_description" ;;
esac
awk -v version="$glibc_version" 'BEGIN {
    split(version, parts, ".")
    exit ! (parts[1] > 2 || (parts[1] == 2 && parts[2] >= 39))
}' || die "the native archive requires glibc 2.39 or newer; detected: $glibc_version"

install -d -o root -g root -m 0755 "$INSTALL_ROOT" "$INSTALL_ROOT/releases"
install_lock="$INSTALL_ROOT/.install.lock"
mkdir "$install_lock" 2>/dev/null || die "another DmxServerManager installation is running"

install_game_toolchains

tmp_dir=$(mktemp -d)

if [ -n "$ARCHIVE_FILE" ]; then
    echo "Installing DmxServerManager $VERSION from a local verified archive..."
    install -m 0600 "$ARCHIVE_FILE" "$tmp_dir/$ASSET"
else
    echo "Downloading DmxServerManager $VERSION..."
    curl --fail --location --proto '=https' --tlsv1.2 \
        --output "$tmp_dir/$ASSET" "$BASE_URL/$ASSET"
fi
expected_checksum=$EXPECTED_ARCHIVE_SHA256
actual_checksum=$(sha256sum "$tmp_dir/$ASSET" | awk '{ print $1 }')
[ "$actual_checksum" = "$expected_checksum" ] || die "release checksum verification failed"

mkdir "$tmp_dir/payload"
tar -tzf "$tmp_dir/$ASSET" \
    | awk '/(^|\/)\.\.($|\/)|^\// { exit 1 }' \
    || die "release archive contains a path outside its root"
tar -tvzf "$tmp_dir/$ASSET" \
    | awk 'substr($0, 1, 1) !~ /[-d]/ { exit 1 }' \
    || die "release archive contains links or special files"
tar --no-same-owner --no-same-permissions -xzf "$tmp_dir/$ASSET" -C "$tmp_dir/payload"

if find "$tmp_dir/payload" -type l -o -type b -o -type c -o -type p -o -type s | grep -q .; then
    die "release archive contains a prohibited special file"
fi
[ -f "$tmp_dir/payload/dmx-server-manager" ] \
    || die "release archive does not contain dmx-server-manager"
[ -f "$tmp_dir/payload/static/index.html" ] \
    || die "release archive does not contain static/index.html"
[ -d "$tmp_dir/payload/static/assets" ] \
    || die "release archive does not contain static/assets"
find "$tmp_dir/payload/static/assets" -type f -print -quit | grep -q . \
    || die "release archive contains an empty static/assets directory"

if ! getent group "$SERVICE_GROUP" >/dev/null 2>&1; then
    groupadd --system "$SERVICE_GROUP"
fi
service_group_id=$(getent group "$SERVICE_GROUP" | awk -F: 'NR == 1 { print $3 }')
[ -n "$service_group_id" ] || die "unable to resolve the dedicated service group"
[ "$service_group_id" -ne 0 ] || die "the dedicated service group must not be gid 0"
if ! id "$SERVICE_USER" >/dev/null 2>&1; then
    useradd --system --gid "$SERVICE_GROUP" --home-dir "$DATA_DIR" \
        --shell /usr/sbin/nologin "$SERVICE_USER"
fi

service_user_id=$(getent passwd "$SERVICE_USER" | awk -F: 'NR == 1 { print $3 }')
service_primary_group_id=$(getent passwd "$SERVICE_USER" | awk -F: 'NR == 1 { print $4 }')
[ -n "$service_user_id" ] && [ -n "$service_primary_group_id" ] \
    || die "unable to resolve the dedicated service account"
[ "$service_user_id" -ne 0 ] || die "the dedicated service user must not be uid 0"
[ "$service_primary_group_id" = "$service_group_id" ] \
    || die "the dedicated service user's primary group must be $SERVICE_GROUP"
[ "$(id -G "$SERVICE_USER")" = "$service_group_id" ] \
    || die "the dedicated service user must not belong to supplementary groups"

install -d -o "$SERVICE_USER" -g "$SERVICE_GROUP" -m 0700 \
    "$DATA_DIR" "$DATA_DIR/instances" "$DATA_DIR/backups" "$DATA_DIR/toolchains"
install -d -o root -g "$SERVICE_GROUP" -m 0750 "$CONFIG_DIR"

if [ -L "$CONFIG_DIR/master.key" ] \
    || { [ -e "$CONFIG_DIR/master.key" ] && [ ! -f "$CONFIG_DIR/master.key" ]; }; then
    die "refusing a linked or non-regular master key: $CONFIG_DIR/master.key"
fi
if [ ! -e "$CONFIG_DIR/master.key" ]; then
    openssl rand 32 > "$CONFIG_DIR/master.key"
fi
[ "$(wc -c < "$CONFIG_DIR/master.key" | tr -d ' ')" -eq 32 ] \
    || die "the master key must contain exactly 32 bytes"
chown root:"$SERVICE_GROUP" "$CONFIG_DIR/master.key"
chmod 0640 "$CONFIG_DIR/master.key"

if [ -L "$CONFIG_DIR/config.toml" ] \
    || { [ -e "$CONFIG_DIR/config.toml" ] && [ ! -f "$CONFIG_DIR/config.toml" ]; }; then
    die "refusing a linked or non-regular configuration file: $CONFIG_DIR/config.toml"
fi
if [ ! -e "$CONFIG_DIR/config.toml" ]; then
    install -o root -g "$SERVICE_GROUP" -m 0640 /dev/null "$CONFIG_DIR/config.toml"
    printf '%s\n' \
        'bind = "127.0.0.1:5500"' \
        "data_dir = \"$DATA_DIR\"" \
        "database_url = \"sqlite://$DATA_DIR/dmx-server-manager.sqlite?mode=rwc\"" \
        "master_key_file = \"$CONFIG_DIR/master.key\"" \
        "steamcmd_path = \"$STEAMCMD_PATH\"" \
        "static_dir = \"$INSTALL_ROOT/current/static\"" \
        'reverse_proxy = false' \
        'trusted_proxies = []' \
        'import_roots = []' \
        'log = "info"' \
        'deployment_mode = "native"' \
        '# Official release URL and Ed25519 public key are compiled into the binary.' \
        '# Override release_manifest_url and release_public_key together only.' \
        > "$CONFIG_DIR/config.toml"
fi

release_id="$VERSION-$actual_checksum"
release_dir="$INSTALL_ROOT/releases/$release_id"
if [ -e "$release_dir" ]; then
    [ -d "$release_dir" ] || die "immutable release path is not a directory: $release_dir"
    [ "$(sed -n '1p' "$release_dir/.archive.sha256" 2>/dev/null)" = "$actual_checksum" ] \
        || die "existing immutable release has a different digest"
    [ -x "$release_dir/dmx-server-manager" ] \
        || die "existing immutable release has no executable"
    [ -f "$release_dir/static/index.html" ] \
        || die "existing immutable release has no frontend"
else
    staging_dir="$INSTALL_ROOT/releases/.staging-$release_id-$$"
    mkdir "$staging_dir"
    install -o root -g root -m 0755 \
        "$tmp_dir/payload/dmx-server-manager" "$staging_dir/dmx-server-manager"
    install -d -o root -g root -m 0755 "$staging_dir/static"
    cp -R "$tmp_dir/payload/static/." "$staging_dir/static/"
    chown -R root:root "$staging_dir/static"
    find "$staging_dir/static" -type d -exec chmod 0755 {} \;
    find "$staging_dir/static" -type f -exec chmod 0644 {} \;
    printf '%s\n' "$actual_checksum" > "$staging_dir/.archive.sha256"
    chmod 0444 "$staging_dir/.archive.sha256"
    mv "$staging_dir" "$release_dir"
    staging_dir=''
fi

if [ -e "$INSTALL_ROOT/current" ] && [ ! -L "$INSTALL_ROOT/current" ]; then
    die "$INSTALL_ROOT/current exists and is not a symbolic link"
fi
if [ -L "$INSTALL_ROOT/current" ]; then
    previous_release=$(readlink "$INSTALL_ROOT/current")
fi
if systemctl is-active --quiet dmx-server-manager.service 2>/dev/null; then
    service_was_active=true
fi
if systemctl is-enabled --quiet dmx-server-manager.service 2>/dev/null; then
    service_was_enabled=true
fi
if [ -e "$SERVICE_FILE" ]; then
    service_unit_existed=true
    cp -p "$SERVICE_FILE" "$tmp_dir/service.backup"
fi
if [ -e "$CLI_LINK" ] || [ -L "$CLI_LINK" ]; then
    if [ ! -L "$CLI_LINK" ] || [ "$(readlink "$CLI_LINK")" != "$INSTALL_ROOT/current/dmx-server-manager" ]; then
        die "refusing to overwrite unrelated $CLI_LINK"
    fi
fi

cat > "$tmp_dir/dmx-server-manager.service" <<EOF
# Managed by the DmxServerManager installer.
[Unit]
Description=DmxServerManager game server manager
Documentation=https://github.com/thefrcrazy/DmxServerManager
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_GROUP
WorkingDirectory=$INSTALL_ROOT/current
Environment=DMX_CONFIG_FILE=$CONFIG_DIR/config.toml
Environment=DMX_DATA_DIR=$DATA_DIR
Environment=DMX_MASTER_KEY_FILE=$CONFIG_DIR/master.key
Environment=DMX_STATIC_DIR=$INSTALL_ROOT/current/static
Environment=DMX_STEAMCMD_PATH=$STEAMCMD_PATH
Environment=DMX_DEPLOYMENT_MODE=native
Environment=HOME=$DATA_DIR
Environment=PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
ExecStart=$INSTALL_ROOT/current/dmx-server-manager
Restart=on-failure
RestartSec=5s
TimeoutStartSec=90s
TimeoutStopSec=120s
KillMode=control-group
UMask=0077
AmbientCapabilities=
CapabilityBoundingSet=
NoNewPrivileges=true
PrivateTmp=true
ProtectClock=true
ProtectControlGroups=true
ProtectHome=true
ProtectHostname=true
ProtectKernelLogs=true
ProtectKernelModules=true
ProtectKernelTunables=true
ProtectProc=invisible
ProtectSystem=strict
ReadWritePaths=$DATA_DIR
RemoveIPC=true
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
RestrictSUIDSGID=true
LockPersonality=true

[Install]
WantedBy=multi-user.target
EOF

transaction_started=true
if [ "$service_was_active" = true ]; then
    systemctl stop dmx-server-manager.service
fi

install -o root -g root -m 0644 "$tmp_dir/dmx-server-manager.service" "$SERVICE_FILE"
rm -f "$INSTALL_ROOT/current.new"
ln -s "$release_dir" "$INSTALL_ROOT/current.new"
mv -Tf "$INSTALL_ROOT/current.new" "$INSTALL_ROOT/current"
if [ ! -e "$CLI_LINK" ] && [ ! -L "$CLI_LINK" ]; then
    ln -s "$INSTALL_ROOT/current/dmx-server-manager" "$CLI_LINK"
    cli_link_created=true
fi

systemctl daemon-reload
systemctl enable dmx-server-manager.service >/dev/null
systemctl start dmx-server-manager.service
if ! wait_for_health; then
    die "service health check failed; inspect journalctl -u dmx-server-manager"
fi

if [ "${DMX_NO_START:-0}" = 1 ]; then
    systemctl stop dmx-server-manager.service
fi

transaction_succeeded=true
transaction_started=false

echo "DmxServerManager $VERSION is installed from immutable release $release_id."
if [ "${DMX_NO_START:-0}" = 1 ]; then
    echo "The release passed its HTTP health check and was then stopped (DMX_NO_START=1)."
else
    echo "Open http://localhost:5500 locally to create the first Owner."
fi
echo "Remote access requires TLS or an explicitly declared reverse proxy."
