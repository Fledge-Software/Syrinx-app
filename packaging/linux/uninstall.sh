#!/bin/sh
# Removes the per-user Syrinx that install.sh put under ~/.local. Your data is always kept;
# the message at the end names its folder if you want to delete it yourself.
set -eu

fail() {
    printf 'Syrinx was not uninstalled: %s\n' "$1" >&2
    exit 1
}

[ "$(id -u)" -ne 0 ] || fail "run this as yourself, not as root or with sudo."
[ -n "${HOME:-}" ] || fail "HOME is not set."

bin_dir="$HOME/.local/bin"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
entry="$data_home/applications/syrinx.desktop"
icon="$data_home/icons/hicolor/256x256/apps/syrinx.png"
program="$bin_dir/syrinx"
marker='X-Syrinx-Packaging=syrinx-packaging-v1'

# install.sh writes the desktop entry last, so without it there is nothing of ours to remove.
if [ ! -e "$entry" ]; then
    printf 'Syrinx is not installed for this user (no %s).\n' "$entry"
    exit 0
fi
grep -qxF "$marker" "$entry" || fail "$entry was not written by install.sh, so nothing was removed."

# Remove the program and icon first and the entry last, so an interrupted run can be retried.
rm -f -- "$program" "$bin_dir/.syrinx.new" "$icon"
rm -f -- "$entry"

printf 'Removed Syrinx from %s.\n' "$bin_dir"
printf 'Your data in %s was kept; delete that folder yourself to erase it.\n' "$data_home/Syrinx/data"
