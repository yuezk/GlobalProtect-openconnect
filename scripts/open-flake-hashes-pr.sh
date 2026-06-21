#!/usr/bin/env bash

set -euo pipefail

release_version="${1:-}"

if git diff --quiet -- flake.nix; then
  echo "flake.nix hashes are already up to date"
  exit 0
fi

if [[ -z "$release_version" ]]; then
  release_version="$(grep '^version' Cargo.toml | head -1 | sed 's/version *= *"\(.*\)"/\1/')"
fi

version="${release_version#v}"
branch="chore/update-flake-hashes-v${version}"
title="chore: update flake hashes for v${version}"
body="Updates the Nix flake fetchzip hashes for v${version} release assets."

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
