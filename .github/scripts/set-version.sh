#!/usr/bin/env bash
# Commits the release version on top of the checked-out main commit; main itself is never pushed.
set -euo pipefail
version=$1

# Change only the [workspace.package] version, then let Cargo record it for workspace crates.
# No end-of-line anchor: the Windows runner checks files out with CRLF line endings.
sed -i "0,/^version = \"[^\"]*\"/s//version = \"$version\"/" Cargo.toml
cargo update --workspace --offline

if ! grep -q "^version = \"$version\"" Cargo.toml; then
  echo "::error::Cargo.toml does not carry version $version after the update."
  exit 1
fi

# Refuse anything beyond the two version files; none when main already has this version.
changed=$(git diff --name-only | grep -v -x -E 'Cargo\.(toml|lock)' || true)
if [ -n "$changed" ]; then
  echo "::error::Setting the version changed unexpected files: $changed"
  exit 1
fi

# The release commit always exists, so a tag's parent is the main commit it was built from.
# Identity is per command, so running this locally never changes the checkout's git config.
git -c user.name="github-actions[bot]" \
  -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
  commit --quiet --allow-empty -am "Release Syrinx $version preview"
