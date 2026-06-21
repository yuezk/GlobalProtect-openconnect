#!/usr/bin/env bash

set -euo pipefail

tag="${1:-}"

if [[ -z "$tag" ]]; then
  echo "Usage: $0 <tag>" >&2
  exit 1
fi

version="${tag#v}"

awk -v version="$version" '
  $0 == "## " version {
    found = 1
    next
  }
  found && /^## / {
    exit
  }
  found {
    print
  }
  END {
    if (!found) {
      exit 1
    }
  }
' changelog.md
