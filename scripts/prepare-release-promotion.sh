#!/usr/bin/env bash
# Prepare a reviewable main candidate; publishing and merging remain explicit.
set -euo pipefail

tag="${1:?Usage: prepare-release-promotion.sh <vX.Y.Z> <updated-flake-directory>}"
artifact_dir="$(cd "${2:?Updated flake artifact directory is required}" && pwd)"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "A stable release tag is required" >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "Start with a clean working tree" >&2
  exit 1
fi

python3 - "$artifact_dir/release-candidate.json" "$tag" <<'PY'
import json, sys
from pathlib import Path
candidate = json.loads(Path(sys.argv[1]).read_text())
if candidate['releaseTag'] != sys.argv[2] or candidate['version'] != sys.argv[2][1:]:
    raise SystemExit('Flake artifact does not belong to the requested release')
PY
python3 "$script_dir/validate-release-version.py" --root "$artifact_dir" --version "${tag#v}"

git fetch origin refs/heads/main:refs/remotes/origin/main
git fetch origin "refs/tags/$tag"
release_sha="$(git rev-parse 'FETCH_HEAD^{commit}')"
python3 - "$tag" <<'PY'
import re, subprocess, sys
cargo = subprocess.check_output(['git', 'show', 'origin/main:Cargo.toml'], text=True)
version = re.search(r'^version\s*=\s*"([0-9.]+)"', cargo, re.M)[1]
if tuple(map(int, sys.argv[1][1:].split('.'))) < tuple(map(int, version.split('.'))):
    raise SystemExit('Refusing to prepare a release older than main')
PY

branch="chore/promote-$tag"
git switch -c "$branch" origin/main
git merge --no-commit --no-ff "$release_sha"
cp "$artifact_dir/flake.nix" "$artifact_dir/flake.lock" .
python3 "$script_dir/validate-release-version.py" --version "${tag#v}"
# Check against Cargo too: overriding the expectation above is only for artifacts.
python3 "$script_dir/validate-release-version.py"
git add flake.nix flake.lock
if ! git diff --cached --quiet || [[ -f "$(git rev-parse --git-path MERGE_HEAD)" ]]; then
  git commit -m "release: promote $tag sources and Nix packages together"
fi
echo "Prepared $branch. Review the candidate, push normally, and open one PR against main."
