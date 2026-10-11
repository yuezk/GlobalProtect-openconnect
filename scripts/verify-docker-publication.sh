#!/usr/bin/env bash

# Usage: verify-docker-publication.sh <image:tag> <digest-directory> <all|x86_64|arm64>
set -euo pipefail
shopt -s nullglob

image_ref=${1:?Image reference is required}
digest_directory=${2:?Digest directory is required}
architecture=${3:?Architecture selection is required}
image=${image_ref%:*}
digests=("$digest_directory"/*)
case "$architecture" in
  all) expected='["linux/amd64","linux/arm64"]' ;;
  x86_64) expected='["linux/amd64"]' ;;
  arm64) expected='["linux/arm64"]' ;;
  *) echo "Invalid architecture selection: $architecture" >&2; exit 1 ;;
esac
[[ ${#digests[@]} -eq $(jq length <<<"$expected") ]]

# Buildx can return an OCI index per architecture when provenance is enabled.
# Compare the contained image digests, excluding attestation manifests.
source_digests=()
for file in "${digests[@]}"; do
  digest="$(basename "$file")"
  [[ "$digest" =~ ^[0-9a-f]{64}$ ]]
  source="$(docker buildx imagetools inspect "$image@sha256:$digest" --raw)"
  if jq -e '.manifests' <<<"$source" >/dev/null; then
    images="$(jq -r '.manifests[] | select(.platform.os == "linux" and
      (.platform.architecture == "amd64" or .platform.architecture == "arm64")) | .digest' <<<"$source")"
    [[ -n "$images" ]]
    while IFS= read -r image_digest; do
      source_digests+=("$image_digest")
    done <<< "$images"
  else
    source_digests+=("sha256:$digest")
  fi
done
sources="$(printf '%s\n' "${source_digests[@]}" | jq -R . | jq -s 'sort | unique')"

for _ in {1..20}; do
  if manifest="$(docker buildx imagetools inspect "$image_ref" --raw)" &&
     jq -e --argjson expected "$expected" --argjson sources "$sources" '
       [.manifests[] | select(.platform.os == "linux")] as $images |
       ([$images[] | .platform.os + "/" + .platform.architecture] | sort) == ($expected | sort) and
       ([$images[].digest] | sort | unique) == $sources
     ' <<<"$manifest" >/dev/null; then
    echo "Verified $image_ref: $expected"
    if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
      echo "Verified $image_ref: $expected" >> "$GITHUB_STEP_SUMMARY"
    fi
    exit 0
  fi
  sleep 15
done
echo "Docker publication did not match the expected architectures and build digests: $image_ref" >&2
exit 1
