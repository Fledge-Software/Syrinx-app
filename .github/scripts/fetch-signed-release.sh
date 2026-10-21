#!/usr/bin/env bash
# Downloads a release's signed manifest and latest statement, so a quiet week can re-sign the
# statement before it expires (#55). A release published before signing began has neither.
set -euo pipefail
tag=$1
folder=$2

# Write one key=value line to the step's outputs.
output() { echo "$1=$2" >> "$GITHUB_OUTPUT"; }

assets=$(gh release view "$tag" --json assets --jq '.assets[].name')
for name in manifest latest; do
  if ! grep -qx "$name" <<< "$assets"; then
    echo "::notice::$tag has no signed update files ($name is missing), so there is no statement to re-sign."
    output found false
    exit 0
  fi
done

mkdir -p "$folder"
gh release download "$tag" --dir "$folder" --pattern manifest --pattern latest
echo "Downloaded $tag's manifest and latest statement into $folder."
output found true
