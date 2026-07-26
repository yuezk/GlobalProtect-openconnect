#!/bin/bash

# Usage: ./scripts/gh-release.sh <tag>

set -euo pipefail
shopt -s nullglob

REPO="yuezk/GlobalProtect-openconnect"
TAG=${1:-}

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
GH_MAX_ATTEMPTS=8
GH_DELETE_MAX_ATTEMPTS=3

if [ -z "$TAG" ]; then
  echo "Usage: ./scripts/gh-release.sh <tag>"
  exit 1
fi

retry_gh() {
  local attempt=1
  local delay

  until "$@"; do
    if (( attempt >= GH_MAX_ATTEMPTS )); then
      echo "GitHub operation failed after $attempt attempts" >&2
      return 1
    fi
    delay=$((attempt * 3))
    echo "GitHub operation failed; retrying in ${delay}s" >&2
    sleep "$delay"
    ((attempt += 1))
  done
}

upload_files() {
  local files=("$@")
  local file

  if [ ${#files[@]} -eq 0 ]; then
    echo "No release assets found to upload"
    exit 1
  fi

  for file in "${files[@]}"; do
    upload_file "$file"
  done
}

release_assets() {
  "$SCRIPT_DIR/release-assets.sh" "$TAG"
}

release_assets_json() {
  retry_gh gh -R "$REPO" release view "$TAG" --json assets
}

release_asset_names() {
  retry_gh gh -R "$REPO" release view "$TAG" --json assets --jq '.assets[].name'
}

delete_release_asset() {
  local asset=$1
  local assets
  local attempt=1
  local delay

  while (( attempt <= GH_DELETE_MAX_ATTEMPTS )); do
    if gh -R "$REPO" release delete-asset "$TAG" "$asset" --yes; then
      return 0
    fi
    if assets="$(release_asset_names)" && ! grep -Fqx -- "$asset" <<<"$assets"; then
      return 0
    fi
    if (( attempt >= GH_DELETE_MAX_ATTEMPTS )); then
      echo "Failed to delete release asset after $attempt attempts: $asset" >&2
      return 1
    fi
    delay=$((attempt * 3))
    echo "Failed to delete $asset; retrying in ${delay}s" >&2
    sleep "$delay"
    ((attempt += 1))
  done
}

upload_file() {
  local file=$1
  local asset
  local assets_json
  local local_digest
  local remote_digest
  local remote_exists
  local attempt=1
  local delay

  asset="$(basename "$file")"
  local_digest="sha256:$(sha256sum "$file" | cut -d ' ' -f 1)"
  echo "Uploading $asset..."

  while (( attempt <= GH_MAX_ATTEMPTS )); do
    if gh -R "$REPO" release upload "$TAG" "$file"; then
      return 0
    fi

    if assets_json="$(release_assets_json)"; then
      remote_digest="$(jq -r --arg name "$asset" \
        '.assets[] | select(.name == $name) | .digest // empty' <<<"$assets_json")"
      if [[ "$remote_digest" == "$local_digest" ]]; then
        echo "$asset is already uploaded with the expected digest"
        return 0
      fi

      remote_exists="$(jq -r --arg name "$asset" \
        '[.assets[] | select(.name == $name)] | length' <<<"$assets_json")"
      if [[ "$remote_exists" != "0" ]]; then
        if ! delete_release_asset "$asset"; then
          echo "Could not replace release asset yet: $asset" >&2
        fi
      fi
    fi

    if (( attempt >= GH_MAX_ATTEMPTS )); then
      echo "Failed to upload release asset after $attempt attempts: $asset" >&2
      return 1
    fi
    delay=$((attempt * 3))
    echo "Failed to upload $asset; retrying in ${delay}s" >&2
    sleep "$delay"
    ((attempt += 1))
  done
}

# Update the existing snapshot release in place to avoid notification spam.
# Preserve its macOS update assets when the current run does not replace them.
release_snapshot() {
  mapfile -t files < <(release_assets)
  local existing_assets
  local includes_macos=false
  local is_current
  local asset
  local file
  for file in "${files[@]}"; do
    if [[ "$(basename "$file")" == GPConnect_*_arm64.* ]]; then
      includes_macos=true
      break
    fi
  done

  echo "Uploading new assets..."
  # Upload first so a transient cleanup failure cannot leave the snapshot
  # release without the current artifacts.
  upload_files "${files[@]}"

  if ! existing_assets="$(release_asset_names)"; then
    echo "::warning::Could not list stale snapshot assets for cleanup" >&2
    return 0
  fi
  while IFS= read -r asset; do
    if [[ -z "$asset" || "$asset" == "appcast.xml" ]]; then
      continue
    fi

    is_current=false
    for file in "${files[@]}"; do
      if [[ "$(basename "$file")" == "$asset" ]]; then
        is_current=true
        break
      fi
    done
    if [[ "$is_current" == "true" ]]; then
      continue
    fi

    if [[ "$includes_macos" == "false" && \
          "$asset" == GPConnect_*_arm64.* ]]; then
      continue
    fi
    if ! delete_release_asset "$asset"; then
      echo "::warning::Could not remove stale snapshot asset: $asset" >&2
    fi
  done <<<"$existing_assets"
}

release_tag() {
  echo "Removing existing release..."
  gh -R "$REPO" release delete "$TAG" --yes --cleanup-tag || true

  echo "Creating release..."
  local release_notes_file
  release_notes_file="$(mktemp)"
  "$SCRIPT_DIR/release-notes.sh" "$TAG" > "$release_notes_file"

  # Upload source tarballs, GUI components, and BSD packages. Other Linux
  # packages are built in `release.yml` from the standalone source tarball.
  gh -R "$REPO" release create "$TAG" \
    --title "$TAG" \
    --notes-file "$release_notes_file"

  mapfile -t files < <(release_assets)
  upload_files "${files[@]}"
}

if [[ $TAG == *"snapshot" ]]; then
  release_snapshot
else
  release_tag
fi
