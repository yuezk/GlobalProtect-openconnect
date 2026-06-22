#!/usr/bin/env bash

set -euo pipefail

release_version="${1:-}"
release_tag="${2:-}"

if git diff --quiet -- flake.nix; then
  echo "flake.nix hashes are already up to date"
  exit 0
fi

if [[ -z "$release_version" ]]; then
  release_version="$(grep '^version' Cargo.toml | head -1 | sed 's/version *= *"\(.*\)"/\1/')"
fi

version="${release_version#v}"
if [[ -z "$release_tag" ]]; then
  release_tag="v$version"
fi

branch_suffix="v${version}"
title="chore: update flake hashes for v${version}"
body="Updates the Nix flake fetchzip hashes for v${version} release assets."

if [[ "$release_tag" != "v$version" ]]; then
  branch_suffix="${branch_suffix}-${release_tag}"
  title="chore: update flake hashes for v${version} from ${release_tag}"
  body="Updates the Nix flake fetchzip hashes for v${version} using ${release_tag} release assets."
fi

branch="chore/update-flake-hashes-${branch_suffix}"

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git switch -C "$branch"
git add flake.nix
git commit -m "$title"
git push --force-with-lease origin "$branch"

if gh pr view "$branch" --json number > /tmp/flake-pr.json 2> /dev/null; then
  pr_number="$(jq -r '.number' /tmp/flake-pr.json)"
  gh pr edit "$pr_number" --title "$title" --body "$body"
else
  gh pr create --title "$title" --body "$body" --base main --head "$branch"
fi
