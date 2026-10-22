#!/usr/bin/env bash
# Find the latest execution of build-macos, retaining builds across unrelated reruns.
set -euo pipefail

repository=$1
run_id=$2
latest_attempt=$3
[[ "$run_id" =~ ^[0-9]+$ && "$latest_attempt" =~ ^[1-9][0-9]*$ ]]

for ((attempt = latest_attempt; attempt >= 1; attempt--)); do
  jobs=$(gh api --paginate "repos/$repository/actions/runs/$run_id/attempts/$attempt/jobs" \
    --jq '.jobs[] | select(.name == "build-macos")' | jq -s .)
  if [[ $(jq length <<<"$jobs") == 0 ]]; then
    continue
  fi
  if ! jq -e 'length == 1 and .[0].status == "completed" and .[0].conclusion == "success"' <<<"$jobs" >/dev/null; then
    echo "The latest macOS job must have completed successfully." >&2
    exit 1
  fi
  artifact_name="macos-release-$run_id.$attempt.macos"
  artifacts=$(gh api --paginate "repos/$repository/actions/runs/$run_id/artifacts" \
    --jq '.artifacts[]' | jq -s --arg name "$artifact_name" '[.[] | select(.name == $name)]')
  if ! jq -e 'length == 1 and .[0].expired == false' <<<"$artifacts" >/dev/null; then
    echo "The latest macOS build must have one unexpired artifact." >&2
    exit 1
  fi
  echo "$artifact_name"
  exit 0
done

echo "No macOS job was found in this run." >&2
exit 1
