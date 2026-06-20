#!/usr/bin/env bash

set -euo pipefail

REPO="yuezk/GlobalProtect-openconnect"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &> /dev/null && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
FLAKE_FILE="$PROJECT_DIR/flake.nix"

usage() {
  echo "Usage: $0 [version]"
  echo
  echo "Updates flake.nix fetchzip hashes from the published GitHub release assets."
  echo "When version is omitted, the workspace package version from Cargo.toml is used."
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if ! command -v nix > /dev/null; then
  echo "nix is required to calculate unpacked fetchzip hashes" >&2
  exit 1
fi

if ! command -v jq > /dev/null; then
  echo "jq is required to read nix prefetch output" >&2
  exit 1
fi

version="${1:-}"
if [[ -z "$version" ]]; then
  version="$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/version *= *"\(.*\)"/\1/')"
fi
version="${version#v}"

if [[ -z "$version" ]]; then
  echo "Could not determine release version" >&2
  exit 1
fi

prefetch_hash() {
  local url="$1"
  local attempt

  for attempt in {1..12}; do
    if nix \
      --extra-experimental-features nix-command \
      store prefetch-file \
      --hash-type sha256 \
      --json \
      --unpack \
      "$url" \
      | jq -r '.hash'; then
      return 0
    fi

    echo "Asset is not ready yet, retrying ($attempt/12): $url" >&2
    sleep 10
  done

  echo "Failed to prefetch release asset: $url" >&2
  return 1
}

release_url() {
  local asset="$1"
  echo "https://github.com/$REPO/releases/download/v$version/$asset"
}

source_hash="$(prefetch_hash "$(release_url "globalprotect-openconnect-$version.tar.gz")")"
gpgui_x86_64_hash="$(prefetch_hash "$(release_url "gpgui_x86_64.bin.tar.xz")")"
gpgui_aarch64_hash="$(prefetch_hash "$(release_url "gpgui_aarch64.bin.tar.xz")")"
binary_x86_64_hash="$(prefetch_hash "$(release_url "globalprotect-openconnect_${version}_x86_64.bin.tar.xz")")"
binary_aarch64_hash="$(prefetch_hash "$(release_url "globalprotect-openconnect_${version}_aarch64.bin.tar.xz")")"

SOURCE_HASH="$source_hash" \
GPGUI_X86_64_HASH="$gpgui_x86_64_hash" \
GPGUI_AARCH64_HASH="$gpgui_aarch64_hash" \
BINARY_X86_64_HASH="$binary_x86_64_hash" \
BINARY_AARCH64_HASH="$binary_aarch64_hash" \
perl -0pi -e '
  s|(url = "https://github\.com/yuezk/GlobalProtect-openconnect/releases/download/v\$\{version\}/globalprotect-openconnect-\$\{version\}\.tar\.gz";\n\s*hash = ")[^"]+(";)|$1 . $ENV{"SOURCE_HASH"} . $2|e;
  s|(gpguiHashes = \{\n\s*x86_64 = ")[^"]+(";)|$1 . $ENV{"GPGUI_X86_64_HASH"} . $2|e;
  s|(gpguiHashes = \{\n\s*x86_64 = "[^"]+";\n\s*aarch64 = ")[^"]+(";)|$1 . $ENV{"GPGUI_AARCH64_HASH"} . $2|e;
  s|(binaryHashes = \{\n\s*x86_64 = ")[^"]+(";)|$1 . $ENV{"BINARY_X86_64_HASH"} . $2|e;
  s|(binaryHashes = \{\n\s*x86_64 = "[^"]+";\n\s*aarch64 = ")[^"]+(";)|$1 . $ENV{"BINARY_AARCH64_HASH"} . $2|e;
' "$FLAKE_FILE"

grep -F "$source_hash" "$FLAKE_FILE" > /dev/null
grep -F "$gpgui_x86_64_hash" "$FLAKE_FILE" > /dev/null
grep -F "$gpgui_aarch64_hash" "$FLAKE_FILE" > /dev/null
grep -F "$binary_x86_64_hash" "$FLAKE_FILE" > /dev/null
grep -F "$binary_aarch64_hash" "$FLAKE_FILE" > /dev/null

echo "Updated flake.nix hashes for v$version"
