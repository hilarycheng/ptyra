#!/bin/sh
set -eu

if [ "$(id -u)" -eq 0 ]; then
    printf '%s\n' 'Run install.sh without sudo; Ptyra installs into your user account.' >&2
    exit 1
fi

binary=${HOME:?HOME is not set}/.local/bin/ptyra
if [ -x "$binary" ]; then
    exec "$binary" --install
fi

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
artifact=$script_dir/ptyra
if [ ! -x "$artifact" ]; then
    printf '%s\n' 'Ptyra binary is missing. Run build.sh first, then install.sh.' >&2
    exit 1
fi
if [ -e "$binary" ] || [ -L "$binary" ]; then
    printf 'Refusing to replace existing path: %s\n' "$binary" >&2
    exit 1
fi

data_home=${XDG_DATA_HOME:-$HOME/.local/share}
case $data_home in /*) ;; *) data_home=$HOME/.local/share ;; esac
desktop=$data_home/applications/ptyra.desktop
icon=$data_home/ptyra/ptyra.svg
if [ -e "$desktop" ] || [ -e "$icon" ]; then
    if [ -f "$desktop" ] && [ -f "$icon" ] \
        && grep -Fqx "Exec=\"$binary\"" "$desktop" \
        && grep -Fqx "TryExec=$binary" "$desktop" \
        && grep -Fqx "Icon=$icon" "$desktop"; then
        install -m 0755 "$artifact" "$binary"
        printf 'Installed %s\n' "$binary"
        exit 0
    fi
    printf '%s\n' 'Existing Ptyra launcher files do not match; refusing to replace them.' >&2
    exit 1
fi

exec "$artifact" --install
