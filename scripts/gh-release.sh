#!/bin/bash

# Usage: ./scripts/gh-release.sh <tag>

set -euo pipefail
shopt -s nullglob

REPO="yuezk/GlobalProtect-openconnect"
TAG=${1:-}

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
GH_MAX_ATTEMPTS=5

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

  if [ ${#files[@]} -eq 0 ]; then
    echo "No release assets found to upload"
    exit 1
  fi

  retry_gh gh -R "$REPO" release upload "$TAG" --clobber "${files[@]}"
}

release_assets() {
  "$SCRIPT_DIR/release-assets.sh" "$TAG"
}

snapshot_asset_names() {
  retry_gh gh -R "$REPO" release view "$TAG" --json assets --jq '.assets[].name'
}

delete_snapshot_asset() {
  local asset=$1
  local assets
  local attempt=1
  local delay

  while (( attempt <= GH_MAX_ATTEMPTS )); do
    if gh -R "$REPO" release delete-asset "$TAG" "$asset" --yes; then
      return 0
    fi
    if assets="$(snapshot_asset_names)" && ! grep -Fqx -- "$asset" <<<"$assets"; then
      return 0
    fi
    if (( attempt >= GH_MAX_ATTEMPTS )); then
      echo "Failed to delete release asset after $attempt attempts: $asset" >&2
      return 1
    fi
    delay=$((attempt * 3))
    echo "Failed to delete $asset; retrying in ${delay}s" >&2
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
  local file
  for file in "${files[@]}"; do
    if [[ "$(basename "$file")" == GP-Connect-*-arm64.* ]]; then
      includes_macos=true
      break
    fi
  done

  existing_assets="$(snapshot_asset_names)"
  while IFS= read -r asset; do
    if [[ -z "$asset" ]]; then
      continue
    fi
    if [[ "$includes_macos" == "false" && \
          ( "$asset" == "appcast.xml" || "$asset" == GP-Connect-*-arm64.* ) ]]; then
      continue
    fi
    delete_snapshot_asset "$asset"
  done <<<"$existing_assets"

  echo "Uploading new assets..."
  # Upload all artifacts for snapshot release because we don't need to guarantee stability.
  upload_files "${files[@]}"
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
