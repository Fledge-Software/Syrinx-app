#!/bin/sh
# Points packaging/arch/syrinx-bin/PKGBUILD at a published release and rewrites .SRCINFO.
# Usage: packaging/arch/update-pkgbuild.sh 0.9.0   (needs gh, sha256 from the release's sums)
set -eu
version=$1
here=$(CDPATH='' cd -- "$(dirname -- "$0")/syrinx-bin" && pwd)
repo=Fledge-Software/Syrinx-app

sums=$(gh release download "v$version" --repo "$repo" --pattern SHA256SUMS.txt --output -)
line=$(printf '%s\n' "$sums" | grep -- '-linux-x64\.tar\.gz$') ||
    { echo "Release v$version has no Linux tarball." >&2; exit 1; }
sha256=${line%% *}
asset=${line##* }
commit=${asset#Syrinx-"$version"-}
commit=${commit%-linux-x64.tar.gz}

sed -i \
    -e "s/^pkgver=.*/pkgver=$version/" \
    -e "s/^_commit=.*/_commit=$commit/" \
    -e "s/^pkgrel=.*/pkgrel=1/" \
    -e "s/^sha256sums=.*/sha256sums=('$sha256')/" \
    "$here/PKGBUILD"
(cd "$here" && makepkg --printsrcinfo > .SRCINFO)
echo "PKGBUILD now builds $asset ($sha256)."
