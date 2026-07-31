#!/bin/bash

# Usage: ./scripts/upload-release-assets.sh <tag> <file> [file...]

set -euo pipefail

REPOSITORY="${GITHUB_REPOSITORY:-yuezk/GlobalProtect-openconnect}"
TAG="${1:-}"
MAX_ATTEMPTS=8

if [[ -z "$TAG" || "$#" -lt 2 ]]; then
  echo "Usage: $0 <tag> <file> [file...]" >&2
  exit 2
fi
shift

file_digest() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

upload_immutable() {
  local file=$1
  local asset
  local assets_json
  local local_digest
  local remote_digest
  local remote_exists
  local attempt=1
  local delay

  if [[ ! -f "$file" ]]; then
    echo "Release asset does not exist: $file" >&2
    return 1
  fi
  asset="$(basename "$file")"
  local_digest="sha256:$(file_digest "$file")"

  while (( attempt <= MAX_ATTEMPTS )); do
    assets_json="$(gh release view "$TAG" --repo "$REPOSITORY" --json assets)"
    remote_exists="$(jq -r --arg name "$asset" \
      '[.assets[] | select(.name == $name)] | length' <<<"$assets_json")"
    if [[ "$remote_exists" != "0" ]]; then
      remote_digest="$(jq -r --arg name "$asset" \
        '.assets[] | select(.name == $name) | .digest // empty' <<<"$assets_json")"
      if [[ "$remote_digest" == "$local_digest" ]]; then
        echo "$asset is already uploaded with the expected digest"
        return 0
      fi
      echo "Refusing to replace immutable release asset: $asset" >&2
      return 1
    fi

    if gh release upload "$TAG" "$file" --repo "$REPOSITORY"; then
      return 0
    fi
    if (( attempt >= MAX_ATTEMPTS )); then
      echo "Failed to upload release asset after $attempt attempts: $asset" >&2
      return 1
    fi
    delay=$((attempt * 3))
    echo "GitHub upload failed; retrying in ${delay}s" >&2
    sleep "$delay"
    ((attempt += 1))
  done
}

for file in "$@"; do
  upload_immutable "$file"
done
