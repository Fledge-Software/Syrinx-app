#!/usr/bin/env bash
# Commits the release version on top of the checked-out main commit; main itself is never pushed.
set -euo pipefail
version=$1

# Change only the [workspace.package] version, then let Cargo record it for workspace crates.
sed -i "0,/^version = \".*\"$/s//version = \"$version\"/" Cargo.toml
cargo update --workspace --offline

# Refuse anything beyond the two version files.
changed=$(git diff --name-only | sort | tr '\n' ' ')
if [ "$changed" != "Cargo.lock Cargo.toml " ]; then
  echo "::error::Setting the version changed unexpected files: $changed"
  exit 1
fi

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git commit --quiet -am "Release Syrinx $version preview"
