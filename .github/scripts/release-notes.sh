#!/usr/bin/env bash
# Prints Markdown release notes from main's first-parent history since the previous preview,
# and the engines underneath this build: the crate versions and the WebView2 runtime it was tested on.
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

# The version Cargo.lock pins for one package, read from the lock text on standard input.
# Carriage returns are dropped: the Windows runner checks files out with CRLF line endings.
locked() {
  tr -d '\r' | awk -v name="$1" '$0 == "name = \"" name "\"" { getline; gsub(/version = |"/, ""); print; exit }'
}

# The Evergreen WebView2 runtime installed on this machine, which the gate and screenshots ran on.
runtime_version() {
  local key='SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
  local hive
  for hive in HKLM HKCU; do
    MSYS2_ARG_CONV_EXCL='*' reg query "$hive"'\'"$key" /v pv 2>/dev/null | tr -d '\r' |
      awk '$1 == "pv" { print $3; found = 1 } END { exit !found }' && return
  done
  echo "unknown"
}

# One line per engine crate, saying when it changed since the previous preview.
current_lock=$(cat Cargo.lock)
previous_lock=""
if [ -n "$previous_tag" ]; then
  previous_lock=$(git show "$previous_tag:Cargo.lock" 2>/dev/null || true)
fi
engine() {
  local label=$1 package=$2 now before
  now=$(locked "$package" <<<"$current_lock")
  before=$(locked "$package" <<<"$previous_lock")
  if [ -n "$before" ] && [ "$before" != "$now" ]; then
    echo "- $label \`$now\` (updated from \`$before\`)"
  else
    echo "- $label \`${now:-unknown}\`"
  fi
}
engines=$(
  engine "GPUI, which draws Syrinx's interface:" gpui-pre
  engine "gpui-component:" gpui-component
  engine "wry, which drives the page web views:" wry
  engine "webview2-com:" webview2-com
)

cat <<EOF
Weekly preview of Syrinx for Windows 11 (x64), built from \`${source:0:12}\` on \`main\`.

This is an early, unsigned preview. Windows SmartScreen may warn before it runs.
Download the \`-setup.exe\` file, check it against \`SHA256SUMS.txt\`, then run
it: it installs Syrinx for your Windows user, and running a newer one later
updates that install. The zip is the same program for manual installs; see the
[installation guide](https://docs.syrinxapp.dev/getting-started/install/).

## What changed

${changes:-- Maintenance only; see the full list below.}

## Engines

$engines
- Microsoft Edge WebView2 runtime, which renders pages: tested on \`$(runtime_version)\`.
  Syrinx uses the runtime Windows keeps up to date, so yours may be newer.

[Full list of changes]($compare)
EOF
