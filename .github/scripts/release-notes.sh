#!/usr/bin/env bash
# Prints Markdown release notes from main's first-parent history since the previous preview.
set -euo pipefail
previous_tag=$1
source=$2
version=$3
repository=$4

# Drop sync merges and one-word subjects that tell a reader nothing.
readable() { grep -v -E "^- (Merge branch '|Merge remote-tracking |.{0,8}$)" || true; }

# The commits main gained since the previous release was cut, newest first.
if [ -n "$previous_tag" ]; then
  range="$(git rev-parse "$previous_tag^{commit}^")..$source"
  changes=$(git log --first-parent --no-decorate --format='- %s' "$range" | readable)
  compare="https://github.com/$repository/compare/$previous_tag...v$version"
else
  changes=$(git log --first-parent --no-decorate --format='- %s' -n 25 "$source" | readable)
  compare="https://github.com/$repository/commits/$source"
fi

cat <<EOF
Weekly preview of Syrinx for Windows 11 (x64), built from \`${source:0:12}\` on \`main\`.

This is an early, unsigned preview. Windows SmartScreen may warn before it runs.
Download the \`-setup.exe\` file, check it against \`SHA256SUMS.txt\`, then run
it: it installs Syrinx for your Windows user, and running a newer one later
updates that install. The zip is the same program for manual installs; see the
[installation guide](https://docs.syrinxapp.dev/getting-started/install/).

## What changed

${changes:-- Maintenance only; see the full list below.}

[Full list of changes]($compare)
EOF
