#!/usr/bin/env bash
# Decides whether this run signs: only once the built source pins a release key (#55).
# Before that every run stays unsigned with a warning; after it, a missing secret fails the run.
set -euo pipefail

# Write one key=value line to the step's outputs.
output() { echo "$1=$2" >> "$GITHUB_OUTPUT"; }

# A failing command stops the script here; its output is counted only once it succeeded.
keys=$(cargo xtask pinned-release-keys)
pinned=$(awk '/^[0-9a-f]{16} [0-9a-f]{128}$/ { count++ } END { print count + 0 }' <<< "$keys")

if [ "$pinned" -eq 0 ]; then
  echo "::warning::No release key is pinned in PRODUCTION_KEYS yet, so this run signs no update (#55)."
  output ready false
  exit 0
fi

if [ "${HAS_RELEASE_KEY:-false}" != "true" ]; then
  echo "::error::This build pins $pinned release key(s) but the SYRINX_RELEASE_KEY secret is not available to this job; check the release-signing environment."
  exit 1
fi

echo "This build pins $pinned release key(s); the update will be signed."
output ready true
