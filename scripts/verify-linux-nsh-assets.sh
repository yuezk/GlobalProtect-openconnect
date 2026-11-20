#!/usr/bin/env bash
set -euo pipefail

root="${1:?Expected a release download directory or Actions artifact directory}"
for arch in x86_64 aarch64; do
  for suffix in '' '-musl'; do
    directory="$root"
    if [[ ${2:-} == --actions ]]; then
      directory="$root/artifact-gpgui-nsh${suffix}-$arch"
    fi
    archive="$directory/gpgui-nsh_${arch}${suffix}.bin.tar.xz"
    digest=$(cat "$archive.sha256")
    if [[ ! $digest =~ ^[[:xdigit:]]{64}$ ]]; then
      echo "Invalid SHA-256 sidecar: $archive.sha256" >&2
      exit 1
    fi
    printf '%s  %s\n' "$digest" "$archive" | sha256sum -c - >&2
  done
done
