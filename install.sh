#!/bin/sh
set -eu

if [ "$(id -u)" -eq 0 ]; then
    printf '%s\n' 'Run install.sh without sudo; Ptyra installs into your user account.' >&2
    exit 1
fi

binary=${HOME:?HOME is not set}/.local/bin/ptyra
if [ ! -x "$binary" ]; then
    printf 'Ptyra executable not found: %s\n' "$binary" >&2
    printf '%s\n' 'Build and install Ptyra first, then run this script without sudo.' >&2
    exit 1
fi

exec "$binary" --install
