#!/usr/bin/env bash
# Builds the Linux tarball with `cargo xtask package` from the checked-out release tag, adds it
# to that tag's GitHub release, and folds its checksum into the release's SHA256SUMS.txt.
set -euo pipefail
version=$1
tag=$2

cargo xtask package

# Exactly one clean tarball for this version, or stop before the release is touched.
shopt -s nullglob
tarballs=(target/package/Syrinx-"$version"-*-linux-x64.tar.gz)
if [ "${#tarballs[@]}" -ne 1 ] || [[ "${tarballs[0]}" == *-dirty-linux-x64.tar.gz ]]; then
  echo "::error::Expected one clean Linux tarball for $version, found: ${tarballs[*]:-none}"
  exit 1
fi
tarball=$(basename "${tarballs[0]}")

# The Windows job wrote SHA256SUMS.txt; keep its lines and add the tarball's.
sums=$(mktemp -d)
gh release download "$tag" --pattern SHA256SUMS.txt --dir "$sums"
grep -v -- '-linux-x64\.tar\.gz$' "$sums/SHA256SUMS.txt" > target/package/SHA256SUMS.txt || true
(cd target/package && sha256sum "$tarball" >> SHA256SUMS.txt)

gh release upload "$tag" "target/package/$tarball"
gh release upload "$tag" target/package/SHA256SUMS.txt --clobber

sha256=$(sha256sum "target/package/$tarball" | cut -d' ' -f1)
echo "tarball=$tarball" >> "$GITHUB_OUTPUT"
echo "sha256=$sha256" >> "$GITHUB_OUTPUT"
echo "Added $tarball (SHA-256 $sha256) to $tag"
