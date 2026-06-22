#!/usr/bin/env bash

set -euo pipefail

tag="${1:-}"

if [[ -z "$tag" ]]; then
  echo "Usage: $0 <tag>" >&2
  exit 1
fi

version="${tag#v}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
changelog="$script_dir/../changelog.md"

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
' "$changelog"
