#!/bin/bash

# Usage: ./scripts/upload-release-assets.sh [--clobber] <tag> <file> [file...]

set -euo pipefail

REPOSITORY="${RELEASE_REPOSITORY:-${GITHUB_REPOSITORY:-yuezk/GlobalProtect-openconnect}}"
MAX_ATTEMPTS=8
CLOBBER=false

if [[ "${1:-}" == "--clobber" ]]; then
  CLOBBER=true
  shift
fi
TAG="${1:-}"

if [[ -z "$TAG" || "$#" -lt 2 ]]; then
  echo "Usage: $0 [--clobber] <tag> <file> [file...]" >&2
  exit 2
fi
shift

if [[ -z "${GH_TOKEN:-}" ]]; then
  echo "GH_TOKEN is not configured" >&2
  exit 1
fi
API_HEADERS=(
  -H "Authorization: Bearer $GH_TOKEN"
  -H 'Accept: application/vnd.github+json'
  -H 'X-GitHub-Api-Version: 2022-11-28'
)
RELEASE_ID="$(curl --fail --silent --show-error \
  "${API_HEADERS[@]}" \
  "https://api.github.com/repos/$REPOSITORY/releases/tags/$TAG" | jq -r .id)"

file_digest() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

upload_asset() {
  local file=$1
  local asset
  local asset_id
  local assets_json
  local encoded_asset
  local local_digest
  local remote_digest
  local attempt=1
  local delay

  if [[ ! -f "$file" ]]; then
    echo "Release asset does not exist: $file" >&2
    return 1
  fi
  asset="$(basename "$file")"
  encoded_asset="$(jq -rn --arg name "$asset" '$name | @uri')"
  local_digest="sha256:$(file_digest "$file")"

  while (( attempt <= MAX_ATTEMPTS )); do
    assets_json="$(curl --fail --silent --show-error \
      "${API_HEADERS[@]}" \
      "https://api.github.com/repos/$REPOSITORY/releases/$RELEASE_ID/assets?per_page=100")"
    asset_id="$(jq -r --arg name "$asset" \
      '[.[] | select(.name == $name)] | first | .id // empty' <<<"$assets_json")"
    if [[ -n "$asset_id" ]]; then
      remote_digest="$(jq -r --arg name "$asset" \
        '.[] | select(.name == $name) | .digest // empty' <<<"$assets_json")"
      if [[ "$CLOBBER" == "false" && "$remote_digest" == "$local_digest" ]]; then
        echo "$asset is already uploaded with the expected digest"
        return 0
      fi
      if [[ "$CLOBBER" == "false" ]]; then
        echo "Refusing to replace immutable release asset: $asset" >&2
        return 1
      fi
      curl --fail --silent --show-error \
        "${API_HEADERS[@]}" \
        --request DELETE \
        "https://api.github.com/repos/$REPOSITORY/releases/assets/$asset_id"
    fi

    if curl --fail --silent --show-error \
      "${API_HEADERS[@]}" \
      --request POST \
      -H 'Content-Type: application/octet-stream' \
      --data-binary "@$file" \
      "https://uploads.github.com/repos/$REPOSITORY/releases/$RELEASE_ID/assets?name=$encoded_asset" \
      >/dev/null; then
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
  upload_asset "$file"
done
