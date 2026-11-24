#!/usr/bin/env bash
# Decides whether this run releases, and which 0.N.0 preview version it gets.
set -euo pipefail

# Write one key=value line to the job's outputs.
output() { echo "$1=$2" >> "$GITHUB_OUTPUT"; }

# Skip unless this is the 10:00 New York run (the other cron line covers daylight saving).
on_ten_am_new_york() {
  [ "$EVENT" != "schedule" ] && return 0
  local offset
  offset=$(TZ=America/New_York date +%z)
  case "$SCHEDULE|$offset" in
    "0 14 * * 4|-0400" | "0 15 * * 4|-0500") return 0 ;;
    *) return 1 ;;
  esac
}

# A release commit sits on top of the main commit it was built from.
source_of() { git rev-parse "$1^{commit}^"; }

# Next minor version after the latest tag, or the workspace version for the first release.
next_version() {
  local latest=$1
  if [ -z "$latest" ]; then
    sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1
    return
  fi
  local major minor
  IFS=. read -r major minor _ <<< "${latest#v}"
  echo "$major.$((minor + 1)).0"
}

if ! on_ten_am_new_york; then
  echo "Not 10:00 in New York for this schedule line; the other line releases."
  output release false
  output resign false
  exit 0
fi

source=$(git rev-parse origin/main)
latest=$(git tag --list 'v*' --sort=-v:refname | head -n 1)

output latest_tag "$latest"

# A quiet week still re-signs the latest statement, so it never expires (#55).
if [ -n "$latest" ] && [ "$(source_of "$latest")" = "$source" ] && [ "$FORCE" != "true" ]; then
  echo "main has no commits since $latest; skipping this week's release and re-signing $latest's statement."
  output release false
  output resign true
  exit 0
fi

private=$(gh api "repos/$REPOSITORY" --jq .private)
if [ "$private" = "true" ]; then
  echo "::warning::$REPOSITORY is private, so this release stays on GitHub and syrinxapp.dev is not updated."
fi

version=$(next_version "$latest")
echo "Releasing $version from $source (previous: ${latest:-none})."
output release true
output resign false
output version "$version"
output previous_tag "$latest"
output source "$source"
output public "$([ "$private" = "false" ] && echo true || echo false)"
