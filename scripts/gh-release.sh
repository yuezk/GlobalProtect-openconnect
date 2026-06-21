#!/bin/bash

# Usage: ./scripts/gh-release.sh <tag>

set -euo pipefail
shopt -s nullglob

REPO="yuezk/GlobalProtect-openconnect"
TAG=${1:-}

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"

if [ -z "$TAG" ]; then
  echo "Usage: ./scripts/gh-release.sh <tag>"
  exit 1
fi

upload_files() {
  local files=("$@")

  if [ ${#files[@]} -eq 0 ]; then
    echo "No release assets found to upload"
    exit 1
  fi

  gh -R "$REPO" release upload "$TAG" "${files[@]}"
}

release_assets() {
  "$SCRIPT_DIR/release-assets.sh" "$TAG"
}

# For snapshot release, we don't create a release, just clear the existing assets and upload new ones.
# This is to avoid notification spam.
release_snapshot() {
  while IFS= read -r asset; do
    if [ -n "$asset" ]; then
      gh -R "$REPO" release delete-asset "$TAG" "$asset" --yes
    fi
  done < <(gh -R "$REPO" release view "$TAG" --json assets --jq '.assets[].name')

  echo "Uploading new assets..."
  # Upload all artifacts for snapshot release because we don't need to guarantee stability.
  mapfile -t files < <(release_assets)
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
