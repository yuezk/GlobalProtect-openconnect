#!/bin/bash

set -euo pipefail
shopt -s nullglob

SELECTION=${1:-}

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
ARTIFACT_DIR=${2:-"$PROJECT_DIR/.build/artifacts"}

if [ -z "$SELECTION" ]; then
  echo "Usage: ./scripts/release-assets.sh <snapshot|tag|all|release-tag> [artifact-dir]" >&2
  exit 1
fi

if [[ "$SELECTION" == *"snapshot"* ]]; then
  SELECTION=snapshot
elif [ "$SELECTION" != "all" ]; then
  SELECTION=tag
fi

case "$SELECTION" in
  snapshot|all)
    printf '%s\n' "$ARTIFACT_DIR"/artifact-*/*
    ;;
  tag)
    printf '%s\n' \
      "$ARTIFACT_DIR"/artifact-source*/* \
      "$ARTIFACT_DIR"/artifact-gpgui-*/* \
      "$ARTIFACT_DIR"/artifact-bsd-*/*
    ;;
  *)
    echo "Unknown release asset selection: $SELECTION" >&2
    exit 1
    ;;
esac
