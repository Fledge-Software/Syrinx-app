#!/usr/bin/env bash
# Builds the setup program and the portable zip with `cargo xtask package`, and records their
# checksums. The setup program is the download people run; the zip stays for updates and scripts.
set -euo pipefail
version=$1

cargo xtask package

# Exactly one clean zip and setup program for this version, or stop before anything is published.
shopt -s nullglob
zips=(target/package/Syrinx-"$version"-*-x64.zip)
setups=(target/package/Syrinx-"$version"-*-x64-setup.exe)
if [ "${#zips[@]}" -ne 1 ] || [[ "${zips[0]}" == *-dirty-x64.zip ]]; then
  echo "::error::Expected one clean package zip for $version, found: ${zips[*]:-none}"
  exit 1
fi
if [ "${#setups[@]}" -ne 1 ] || [[ "${setups[0]}" == *-dirty-x64-setup.exe ]]; then
  echo "::error::Expected one clean setup program for $version, found: ${setups[*]:-none}"
  exit 1
fi
zip=$(basename "${zips[0]}")
asset=$(basename "${setups[0]}")

sha256=$(sha256sum "target/package/$asset" | cut -d' ' -f1)
(cd target/package && sha256sum "$asset" "$zip" > SHA256SUMS.txt)
size=$(awk -v bytes="$(stat -c %s "target/package/$asset")" 'BEGIN { printf "%.1f MB", bytes / 1048576 }')

echo "asset=$asset" >> "$GITHUB_OUTPUT"
echo "zip=$zip" >> "$GITHUB_OUTPUT"
echo "sha256=$sha256" >> "$GITHUB_OUTPUT"
echo "size=$size" >> "$GITHUB_OUTPUT"
echo "Packaged $asset ($size), SHA-256 $sha256, and $zip"
