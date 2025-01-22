#!/bin/bash

# Usage: ./scripts/gh-release.sh <tag>

set -e

REPO="yuezk/GlobalProtect-openconnect"
TAG=$1

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

RELEASE_NOTES="Release $TAG"

if [ -z "$TAG" ]; then
  echo "Usage: ./scripts/gh-release.sh <tag>"
  exit 1
fi

# For snapshot release, we don't create a release, just clear the existing assets and upload new ones.
# This is to avoid notification spam.
release_snapshot() {
  RELEASE_NOTES='**!!! DO NOT USE THIS RELEASE IN PRODUCTION !!!**'

  # Get the existing assets
  gh -R "$REPO" release view "$TAG" --json assets --jq '.assets[].name' \
    | xargs -I {} gh -R "$REPO" release delete-asset "$TAG" {} --yes

  echo "Uploading new assets..."
  gh -R "$REPO" release upload "$TAG" \
    "$PROJECT_DIR"/.build/artifacts/artifact-source*/* \
    "$PROJECT_DIR"/.build/artifacts/artifact-gpgui-*/*
}

release_tag() {
  echo "Removing existing release..."
  gh -R "$REPO" release delete $TAG --yes --cleanup-tag || true

  echo "Creating release..."
  gh -R "$REPO" release create $TAG \
    --title "$TAG" \
    --notes "$RELEASE_NOTES" \
    "$PROJECT_DIR"/.build/artifacts/artifact-source*/* \
    "$PROJECT_DIR"/.build/artifacts/artifact-gpgui-*/*
}

if [[ $TAG == *"snapshot" ]]; then
  release_snapshot
else
  release_tag
fi
