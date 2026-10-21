#!/usr/bin/env bash
# Signs with the key imported for this job, then lays out the host's updates/v1/ files, checked
# as an installed Syrinx would check them (#55).
#   release: sign the package's manifest and a new statement, and lay out every file.
#   resign:  sign only a new statement for the already verified manifest; lay out latest only.
set -euo pipefail
mode=$1
package=$2
output=$3

case "$mode" in
  release) command=sign-update files=include ;;
  resign) command=resign-statement files=omit ;;
  *) echo "Usage: sign-update.sh <release|resign> <package folder> <output folder>" >&2; exit 2 ;;
esac

# Unix seconds grow from run to run, so installed copies never see the serial go backwards.
serial=$(date -u +%s)

cargo xtask "$command" --package "$package" --storage software --key "$KEY_NAME" \
  --serial "$serial" --days "$STATEMENT_DAYS"
cargo xtask host-layout --package "$package" --output "$output" --files "$files"
