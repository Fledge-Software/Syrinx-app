#!/usr/bin/env bash
# Signs a package folder with the key imported for this job, then lays out the host's updates/v1/
# files from it, checked as an installed Syrinx would check them (#55).
set -euo pipefail
package=$1
output=$2
files=$3

# Unix seconds grow from run to run, so installed copies never see the serial go backwards.
serial=$(date -u +%s)

cargo xtask sign-update --package "$package" --storage software --key "$KEY_NAME" \
  --serial "$serial" --days "$STATEMENT_DAYS"
cargo xtask host-layout --package "$package" --output "$output" --files "$files"
