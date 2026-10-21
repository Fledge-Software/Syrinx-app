#!/usr/bin/env bash
# Decides whether this run signs the update: the key secret is set and the built source pins a key.
# Until the owner's keys exist (#55) both are missing, and the run publishes no signed update.
set -euo pipefail

# Write one key=value line to the step's outputs.
output() { echo "$1=$2" >> "$GITHUB_OUTPUT"; }

if [ "${HAS_RELEASE_KEY:-false}" != "true" ]; then
  echo "::warning::The SYRINX_RELEASE_KEY secret is not set, so this run signs no update (#55)."
  output ready false
  exit 0
fi

# Each pinned key prints as "<16-hex identifier> <128-hex point>"; anything else is build noise.
pinned=$(cargo xtask pinned-release-keys | grep -cE '^[0-9a-f]{16} [0-9a-f]{128}$' || true)
if [ "$pinned" -eq 0 ]; then
  echo "::warning::No release key is pinned in PRODUCTION_KEYS, so the secret is not used and no update is signed (#55)."
  output ready false
  exit 0
fi

echo "This build pins $pinned release key(s); the update will be signed."
output ready true
