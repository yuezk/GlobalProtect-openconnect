#!/bin/sh

# Wrapper for gpclient hip
LOGFILE="/tmp/gpclient-hipreport.log"

LINUX_GPCLIENT_BIN="/usr/bin/gpclient"
HOMEBREW_GPCLIENT_BIN="/opt/homebrew/bin/gpclient"
GPCLIENT_BIN="${GPCLIENT_BIN:-}"

if [ -n "$GPCLIENT_BIN" ] && [ -x "$GPCLIENT_BIN" ]; then
    :
elif [ -x "$LINUX_GPCLIENT_BIN" ]; then
    GPCLIENT_BIN="$LINUX_GPCLIENT_BIN"
elif [ -x "$HOMEBREW_GPCLIENT_BIN" ]; then
    GPCLIENT_BIN="$HOMEBREW_GPCLIENT_BIN"
else
    echo "Error: gpclient binary not found." > "$LOGFILE"
    exit 1
fi

HIP_REPORT_OUTPUT=$("$GPCLIENT_BIN" hip "$@" 2> "$LOGFILE")
STATUS=$?

if [ $STATUS -ne 0 ]; then
    exit $STATUS
fi

printf '%s\n' "$HIP_REPORT_OUTPUT"
