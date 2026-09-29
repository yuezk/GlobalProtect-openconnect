#!/bin/sh

# Wrapper for gpclient hip

LINUX_GPCLIENT_BIN="/usr/bin/gpclient"
ARM64_HOMEBREW_GPCLIENT_BIN="/opt/homebrew/bin/gpclient"
LOCAL_GPCLIENT_BIN="/usr/local/bin/gpclient"
GPCLIENT_BIN=""

if [ -x "$LINUX_GPCLIENT_BIN" ]; then
    GPCLIENT_BIN="$LINUX_GPCLIENT_BIN"
elif [ -x "$ARM64_HOMEBREW_GPCLIENT_BIN" ]; then
    GPCLIENT_BIN="$ARM64_HOMEBREW_GPCLIENT_BIN"
elif [ -x "$LOCAL_GPCLIENT_BIN" ]; then
    GPCLIENT_BIN="$LOCAL_GPCLIENT_BIN"
else
    echo "Error: gpclient binary not found." >&2
    exit 1
fi

exec "$GPCLIENT_BIN" hip "$@"
