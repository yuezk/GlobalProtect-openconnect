#!/usr/bin/env bash
# Publish a tested snapshot flake beside its source branch; never open a main PR.
set -euo pipefail

source_branch="${GITHUB_REF_NAME:?Run this workflow from the source branch}"
read -r branch expected version < <(python3 - <<'PYMETA'
import json, re
from pathlib import Path
metadata = json.loads(Path('release-candidate.json').read_text())
if metadata['releaseTag'] != 'snapshot':
    raise SystemExit('Only snapshot test branches can be published')
head = metadata['expectedTestHead']
if head and not re.fullmatch('[0-9a-f]{40}', head):
    raise SystemExit('Invalid test branch lease')
print(metadata['testBranch'], head or '-', metadata['version'])
PYMETA
)
if [[ "$expected" == - ]]; then
  expected=""
fi
git check-ref-format "refs/heads/$branch"
python3 scripts/validate-release-version.py --version "$version" --release-tag snapshot
if git diff --quiet -- flake.nix flake.lock; then
  echo "Snapshot flake files are already up to date"
  exit 0
fi

# The lease was captured before candidate generation and Nix validation.
# Concurrent or delayed runs cannot replace a newer branch update.
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git switch -c "$branch"
git add flake.nix flake.lock
git commit -m "test: prepare snapshot flake for $source_branch"
git push --force-with-lease="refs/heads/$branch:$expected" origin "HEAD:refs/heads/$branch"
echo "Published $branch from $source_branch for Nix testing"
