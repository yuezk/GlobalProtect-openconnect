#!/bin/sh
set -eu

repo=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
cd "$repo"

if [ "$(id -u)" -eq 0 ]; then
  echo "Run this script as the desktop user; it invokes sudo to stage helpers and run gpservice." >&2
  exit 1
fi

uid=$(id -u)
if [ -n "${GP_DEV_BOOTSTRAP_SOCKET:-}" ]; then
  socket=$GP_DEV_BOOTSTRAP_SOCKET
else
  socket=/var/run/gpservice-dev-$uid/dev-bootstrap.sock
fi

cargo build -p gpservice -p gpclient

# The privileged resolver requires a root-owned executable and parent directories.
helper_dir=$(sudo mktemp -d /var/run/gpservice-dev-helpers.XXXXXX)
trap 'sudo rm -rf "$helper_dir"' 0
sudo install -m 755 target/debug/gpclient "$helper_dir/gpclient"

echo "Debug credential socket: $socket"
sudo env GP_CLIENT_BINARY="$helper_dir/gpclient" target/debug/gpservice \
  --dev-standalone \
  --dev-uid "$uid" \
  --dev-bootstrap-socket "$socket" \
  "$@"
