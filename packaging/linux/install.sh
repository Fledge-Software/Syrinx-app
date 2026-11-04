#!/bin/sh
# Installs Syrinx for the current user under ~/.local. It needs no root and changes nothing
# outside your home folder; run uninstall.sh from the same package to remove it.
set -eu

fail() {
    printf 'Syrinx was not installed: %s\n' "$1" >&2
    exit 1
}

warn() {
    printf 'Warning: %s\n' "$1" >&2
}

# A per-user install run as root would land in root's home folder.
[ "$(id -u)" -ne 0 ] || fail "run this as yourself, not as root or with sudo."
[ -n "${HOME:-}" ] || fail "HOME is not set."

package=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
bin_dir="$HOME/.local/bin"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
applications="$data_home/applications"
icons="$data_home/icons/hicolor/256x256/apps"
program="$bin_dir/syrinx"

# The desktop entry names the program by its full path; refuse characters it would need escaped.
case $program in
    *[\"\`\$\\%]*) fail "the folder $bin_dir contains a character a desktop entry cannot hold." ;;
esac

# Refuse a damaged or incomplete package before copying anything.
if command -v sha256sum >/dev/null 2>&1; then
    (cd "$package" && sha256sum -c SHA256SUMS >/dev/null) ||
        fail "the package's files do not match its SHA256SUMS; download or unpack it again."
else
    warn "sha256sum is missing, so the package's files were not checked."
fi

# Pages render in WebKitGTK 4.1; warn rather than refuse, because library folders vary.
if command -v ldconfig >/dev/null 2>&1 &&
    ! ldconfig -p 2>/dev/null | grep -q 'libwebkit2gtk-4\.1\.so'; then
    warn "WebKitGTK 4.1 (webkit2gtk-4.1) was not found; Syrinx needs it, with GTK 3, to start."
fi

mkdir -p "$bin_dir" "$applications" "$icons"

# Copy beside the old program, then rename over it, so a running Syrinx keeps its file.
cp "$package/syrinx" "$bin_dir/.syrinx.new"
chmod 755 "$bin_dir/.syrinx.new"
mv -f "$bin_dir/.syrinx.new" "$program"

cp "$package/syrinx.png" "$icons/.syrinx.png.new"
chmod 644 "$icons/.syrinx.png.new"
mv -f "$icons/.syrinx.png.new" "$icons/syrinx.png"

# Write the desktop entry last: uninstall.sh treats it as the record of this install.
while IFS= read -r line; do
    case $line in
        Exec=*) printf 'Exec="%s"\n' "$program" ;;
        *) printf '%s\n' "$line" ;;
    esac
done <"$package/syrinx.desktop" >"$applications/.syrinx.desktop.new"
chmod 644 "$applications/.syrinx.desktop.new"
mv -f "$applications/.syrinx.desktop.new" "$applications/syrinx.desktop"

printf 'Installed Syrinx to %s.\n' "$program"
printf 'It is in your applications menu; your data will live in %s.\n' "$data_home/Syrinx/data"
case ":${PATH:-}:" in
    *":$bin_dir:"*) ;;
    *) printf 'To start it from a terminal as "syrinx", add %s to your PATH.\n' "$bin_dir" ;;
esac
