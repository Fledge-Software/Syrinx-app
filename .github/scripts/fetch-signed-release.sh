#!/usr/bin/env bash
# Downloads a release's four signed files, so a quiet week can verify them and re-sign only the
# statement before it expires (#55). A release published before signing began has none of them.
set -euo pipefail
tag=$1
folder=$2

# Write one key=value line to the step's outputs.
output() { echo "$1=$2" >> "$GITHUB_OUTPUT"; }

assets=$(gh release view "$tag" --json assets --jq '.assets[].name')
for name in manifest manifest.sig latest latest.sig; do
  if ! grep -qx "$name" <<< "$assets"; then
    echo "::notice::$tag has no signed update files ($name is missing), so there is no statement to re-sign."
    output found false
    exit 0
  fi
done

mkdir -p "$folder"
gh release download "$tag" --dir "$folder" \
  --pattern manifest --pattern manifest.sig --pattern latest --pattern latest.sig
echo "Downloaded $tag's signed files into $folder."
output found true
