#!/bin/sh

# Wrapper for gpclient hip
LOGFILE="/tmp/gpclient-hipreport.log"

LINUX_GPCLIENT_BIN="/usr/bin/gpclient"
HOMEBREW_GPCLIENT_BIN="/opt/homebrew/bin/gpclient"
GPCLIENT_BIN=""

if [ -x "$LINUX_GPCLIENT_BIN" ]; then
    GPCLIENT_BIN="$LINUX_GPCLIENT_BIN"
elif [ -x "$HOMEBREW_GPCLIENT_BIN" ]; then
    GPCLIENT_BIN="$HOMEBREW_GPCLIENT_BIN"
else
    echo "Error: gpclient binary not found." > "$LOGFILE"
    exit 1
fi

# Redirect the output to a file for debugging then output to stdout
HIP_REPORT_OUTPUT=$(exec 2> "$LOGFILE" "$GPCLIENT_BIN" hip -vv "$@")

echo "$HIP_REPORT_OUTPUT"
