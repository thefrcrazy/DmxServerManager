#!/bin/sh
set -eu

umask 077

case "$(uname -m)" in
    x86_64|amd64) ;;
    *)
        echo "DmxServerManager 1.0 supports Linux AMD64 only." >&2
        exit 64
        ;;
esac

for directory in "${DMX_DATA_DIR:-/data}" "${HOME:-/data/home}"; do
    if [ ! -d "$directory" ]; then
        mkdir -p "$directory"
    fi
    if [ ! -w "$directory" ]; then
        echo "Directory is not writable by uid $(id -u): $directory" >&2
        exit 73
    fi
done

master_key_file="${DMX_MASTER_KEY_FILE:-/config/master.key}"
if [ ! -r "$master_key_file" ]; then
    echo "Missing readable master key: $master_key_file" >&2
    echo "Run the standalone Docker installer or mount a 32-byte master key read-only." >&2
    exit 78
fi

static_dir="${DMX_STATIC_DIR:-/opt/dmx-server-manager/static}"
if [ ! -r "$static_dir/index.html" ] || [ ! -d "$static_dir/assets" ]; then
    echo "Missing packaged frontend under: $static_dir" >&2
    exit 78
fi

import_roots=${DMX_IMPORT_ROOTS:-}
while [ -n "$import_roots" ]; do
    case "$import_roots" in
        *,*)
            import_root=${import_roots%%,*}
            import_roots=${import_roots#*,}
            ;;
        *)
            import_root=$import_roots
            import_roots=''
            ;;
    esac
    if [ -n "$import_root" ] && { [ ! -d "$import_root" ] || [ ! -r "$import_root" ] || [ ! -x "$import_root" ]; }; then
        echo "Import root is not readable/traversable by uid $(id -u): $import_root" >&2
        exit 77
    fi
done

if [ "$#" -eq 0 ]; then
    set -- /opt/dmx-server-manager/dmx-server-manager
elif [ "${1#-}" != "$1" ]; then
    set -- /opt/dmx-server-manager/dmx-server-manager "$@"
fi

exec "$@"
