#!/bin/sh
set -eu

mode="${1:-direct}"
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
umask 077

for managed_directory in "$script_dir/secrets" "$script_dir/traefik"; do
    if [ -L "$managed_directory" ] || { [ -e "$managed_directory" ] && [ ! -d "$managed_directory" ]; }; then
        echo "Refusing a linked or non-directory managed path: $managed_directory" >&2
        exit 78
    fi
done
if [ -L "$script_dir/.env" ] || { [ -e "$script_dir/.env" ] && [ ! -f "$script_dir/.env" ]; }; then
    echo "Refusing a linked or non-regular Compose environment file: $script_dir/.env" >&2
    exit 78
fi

case "$mode" in
    direct|traefik) ;;
    *)
        echo "Usage: $0 [direct|traefik]" >&2
        exit 64
        ;;
esac

for required_command in openssl cosign; do
    command -v "$required_command" >/dev/null 2>&1 || {
        echo "$required_command is required by the authenticated Docker bootstrap." >&2
        exit 69
    }
done
cosign_version=$(cosign version 2>/dev/null \
    | sed -n 's/^[[:space:]]*GitVersion:[[:space:]]*v\{0,1\}\([^[:space:]]*\).*$/\1/p' \
    | head -n 1)
case "$cosign_version" in
    3.*) ;;
    *)
        echo "Cosign 3.x is required to verify current Sigstore bundles." >&2
        exit 69
        ;;
esac

if [ "$(id -u)" -ne 0 ]; then
    echo "Run this bootstrap with sudo/root so Docker bind mounts keep the" >&2
    echo "master key private to uid/gid 10001 and imports readable by gid 10001." >&2
    exit 77
fi

caller_uid=${SUDO_UID:-0}
caller_gid=${SUDO_GID:-0}

configured_imports=''
configured_image=''
configured_version=''
if [ -f "$script_dir/.env" ]; then
    configured_imports=$(sed -n 's/^DMX_IMPORTS_PATH=//p' "$script_dir/.env" | tail -n 1)
    configured_image=$(sed -n 's/^DMX_IMAGE=//p' "$script_dir/.env" | tail -n 1)
    configured_version=$(sed -n 's/^DMX_VERSION=//p' "$script_dir/.env" | tail -n 1)
fi
imports_setting=${DMX_IMPORTS_PATH:-${configured_imports:-./imports}}
image_setting=${DMX_IMAGE:-${configured_image:-}}
version_setting=${DMX_VERSION:-${configured_version:-}}

case "$imports_setting" in
    '')
        echo "DMX_IMPORTS_PATH cannot be empty." >&2
        exit 64
        ;;
    *[!A-Za-z0-9_./-]*)
        echo "DMX_IMPORTS_PATH contains unsupported characters." >&2
        exit 64
        ;;
esac
case "/$imports_setting/" in
    */../*)
        echo "DMX_IMPORTS_PATH must not contain a parent-directory component." >&2
        exit 64
        ;;
esac

if ! printf '%s\n' "$version_setting" \
    | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
    echo "DMX_VERSION must be the exact semantic version associated with DMX_IMAGE." >&2
    exit 64
fi

case "$image_setting" in
    ghcr.io/thefrcrazy/dmx-server-manager@sha256:*)
        image_digest=${image_setting#*@sha256:}
        case "$image_digest" in *[!0-9a-f]*|'') image_digest_invalid=1 ;; *) image_digest_invalid=0 ;; esac
        if [ "$image_digest_invalid" -ne 0 ] || [ "${#image_digest}" -ne 64 ]; then
            echo "DMX_IMAGE sha256 digest must contain exactly 64 lowercase hexadecimal characters." >&2
            exit 64
        fi
        ;;
    *)
        echo "DMX_IMAGE must be the official image pinned by its signed sha256 digest." >&2
        exit 64
        ;;
esac

certificate_identity="https://github.com/thefrcrazy/DmxServerManager/.github/workflows/release.yml@refs/tags/v$version_setting"
certificate_issuer='https://token.actions.githubusercontent.com'
echo "Verifying the keyless signature for $image_setting..."
cosign verify \
    --certificate-identity "$certificate_identity" \
    --certificate-oidc-issuer "$certificate_issuer" \
    "$image_setting" >/dev/null
echo "Verified DmxServerManager $version_setting image signature and digest."

imports_path=$imports_setting
case "$imports_path" in
    /*) ;;
    *) imports_path="$script_dir/${imports_path#./}" ;;
esac
if [ "$imports_path" = / ]; then
    echo "DMX_IMPORTS_PATH must not be the filesystem root." >&2
    exit 64
fi

mkdir -p "$script_dir/secrets" "$imports_path" "$script_dir/traefik"

if find "$imports_path" -type l -print -quit | grep -q .; then
    echo "The managed imports tree must not contain symbolic links: $imports_path" >&2
    exit 78
fi

master_key="$script_dir/secrets/master.key"
if [ -L "$master_key" ] || { [ -e "$master_key" ] && [ ! -f "$master_key" ]; }; then
    echo "Refusing a linked or non-regular master key: $master_key" >&2
    exit 78
fi
if [ ! -e "$master_key" ]; then
    openssl rand 32 > "$script_dir/secrets/master.key"
    echo "Created install/linux/secrets/master.key. Back it up separately from /data."
fi
if [ "$(wc -c < "$master_key" | tr -d ' ')" -ne 32 ]; then
    echo "The Docker master key must contain exactly 32 bytes: $master_key" >&2
    exit 78
fi
chown 10001:10001 "$master_key"
chmod 0400 "$master_key"

# The panel only reads imports. Keep the tree private to the invoking host user
# and the dedicated container group, without granting any world access.
chown -R "$caller_uid":10001 "$imports_path"
find "$imports_path" -type d -exec chmod 0750 {} \;
find "$imports_path" -type f -exec chmod 0640 {} \;
chown "$caller_uid:$caller_gid" "$script_dir/secrets" "$script_dir/traefik"
chmod 0700 "$script_dir/secrets" "$script_dir/traefik"

if [ ! -e "$script_dir/.env" ]; then
    {
        printf 'DMX_IMAGE=%s\n' "$image_setting"
        printf 'DMX_VERSION=%s\n' "$version_setting"
        printf 'DMX_TIMEZONE=%s\n' "${DMX_TIMEZONE:-Etc/UTC}"
        printf 'DMX_IMPORTS_PATH=%s\n' "${DMX_IMPORTS_PATH:-./imports}"
        if [ "$mode" = "traefik" ]; then
            printf 'DMX_DOMAIN=%s\n' "${DMX_DOMAIN:-}"
            printf 'DMX_ACME_EMAIL=%s\n' "${DMX_ACME_EMAIL:-}"
        fi
    } > "$script_dir/.env"
    chmod 0600 "$script_dir/.env"
    chown "$caller_uid:$caller_gid" "$script_dir/.env"
    echo "Created install/linux/.env. Review it before starting Compose."
fi
chmod 0600 "$script_dir/.env"
chown "$caller_uid:$caller_gid" "$script_dir/.env"

read_env() {
    key="$1"
    sed -n "s/^${key}=//p" "$script_dir/.env" | tail -n 1
}

write_env() {
    key="$1"
    value="$2"
    temporary="$script_dir/.env.tmp.$$"
    if grep -q "^${key}=" "$script_dir/.env"; then
        sed "s/^${key}=.*/${key}=${value}/" "$script_dir/.env" > "$temporary"
        mv "$temporary" "$script_dir/.env"
    else
        printf '%s=%s\n' "$key" "$value" >> "$script_dir/.env"
    fi
    chmod 0600 "$script_dir/.env"
    chown "$caller_uid:$caller_gid" "$script_dir/.env"
}

if [ -n "${DMX_IMPORTS_PATH:-}" ]; then
    write_env DMX_IMPORTS_PATH "$DMX_IMPORTS_PATH"
fi

if [ -n "${DMX_IMAGE:-}" ]; then
    write_env DMX_IMAGE "$DMX_IMAGE"
fi

if [ -n "${DMX_VERSION:-}" ]; then
    write_env DMX_VERSION "$DMX_VERSION"
fi

if [ "$mode" = "traefik" ]; then
    domain="${DMX_DOMAIN:-$(read_env DMX_DOMAIN)}"
    email="${DMX_ACME_EMAIL:-$(read_env DMX_ACME_EMAIL)}"

    case "$domain" in
        ''|*[!A-Za-z0-9.-]*|.*|*.)
            echo "Set a valid DMX_DOMAIN before generating the Traefik configuration." >&2
            exit 64
            ;;
    esac
    case "$email" in
        *[!A-Za-z0-9._+@-]*|'')
            echo "Set a valid DMX_ACME_EMAIL before starting Traefik." >&2
            exit 64
            ;;
    esac
    case "$email" in
        *@*.*) ;;
        *)
            echo "Set a valid DMX_ACME_EMAIL before starting Traefik." >&2
            exit 64
            ;;
    esac

    write_env DMX_DOMAIN "$domain"
    write_env DMX_ACME_EMAIL "$email"
    dynamic_config="$script_dir/traefik/dynamic.yml"
    if [ -L "$dynamic_config" ] || { [ -e "$dynamic_config" ] && [ ! -f "$dynamic_config" ]; }; then
        echo "Refusing a linked or non-regular Traefik configuration: $dynamic_config" >&2
        exit 78
    fi
    sed "s/panel\.example\.com/${domain}/g" \
        "$script_dir/traefik/dynamic.yml.example" \
        > "$dynamic_config"
    chmod 0600 "$dynamic_config"
    chown "$caller_uid:$caller_gid" "$dynamic_config"
    echo "Generated the Traefik route for $domain."
fi
