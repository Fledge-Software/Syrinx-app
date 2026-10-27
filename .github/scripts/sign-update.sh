#!/usr/bin/env bash
# Signs with the key imported for this job, then lays out the host's updates/v1/ files, checked
# as an installed Syrinx would check them (#55).
#   release: sign the package's manifest and a new statement, and lay out every file.
#   resign:  sign only a new statement for the already verified manifest of the tag's version
#            (the fourth argument); lay out latest only.
set -euo pipefail
mode=$1
package=$2
output=$3
version=${4:-}

case "$mode" in
  release) command=(sign-update) files=include ;;
  resign) command=(resign-statement --version "$version") files=omit ;;
  *) echo "Usage: sign-update.sh <release|resign> <package folder> <output folder> [version]" >&2; exit 2 ;;
esac
if [ "$mode" = resign ] && [ -z "$version" ]; then
  echo "A re-sign needs the tag's version." >&2
  exit 2
fi

# Unix seconds grow from run to run, so installed copies never see the serial go backwards.
serial=$(date -u +%s)

cargo xtask "${command[@]}" --package "$package" --storage software --key "$KEY_NAME" \
  --serial "$serial" --days "$STATEMENT_DAYS"
cargo xtask host-layout --package "$package" --output "$output" --files "$files"
