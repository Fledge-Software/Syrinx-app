#!/usr/bin/env bash
# Builds the portable zip with `cargo xtask package` and records its checksum and size.
set -euo pipefail
version=$1

cargo xtask package

# Exactly one clean zip for this version, or stop before anything is published.
shopt -s nullglob
zips=(target/package/Syrinx-"$version"-*-x64.zip)
if [ "${#zips[@]}" -ne 1 ] || [[ "${zips[0]}" == *-dirty-x64.zip ]]; then
  echo "::error::Expected one clean package zip for $version, found: ${zips[*]:-none}"
  exit 1
fi
zip=${zips[0]}
asset=$(basename "$zip")

sha256=$(sha256sum "$zip" | cut -d' ' -f1)
(cd target/package && sha256sum "$asset" > SHA256SUMS.txt)
size=$(awk -v bytes="$(stat -c %s "$zip")" 'BEGIN { printf "%.1f MB", bytes / 1048576 }')

echo "asset=$asset" >> "$GITHUB_OUTPUT"
echo "sha256=$sha256" >> "$GITHUB_OUTPUT"
echo "size=$size" >> "$GITHUB_OUTPUT"
echo "Packaged $asset ($size), SHA-256 $sha256"
