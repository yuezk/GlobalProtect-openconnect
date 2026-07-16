#!/bin/sh
set -eu

repo=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
cd "$repo"

if [ "$(id -u)" -eq 0 ]; then
  echo "Run this script as the desktop user; it invokes sudo only for gpservice." >&2
  exit 1
fi

uid=$(id -u)
if [ -n "${GP_DEV_BOOTSTRAP_SOCKET:-}" ]; then
  socket=$GP_DEV_BOOTSTRAP_SOCKET
else
  socket=/var/run/gpservice-dev-$uid/dev-bootstrap.sock
fi

cargo build -p gpservice
echo "Debug credential socket: $socket"
exec sudo target/debug/gpservice \
  --dev-standalone \
  --dev-uid "$uid" \
  --dev-bootstrap-socket "$socket" \
  "$@"
