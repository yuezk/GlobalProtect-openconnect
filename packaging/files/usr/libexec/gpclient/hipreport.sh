#!/bin/sh

# Wrapper for gpclient hip
GPCLIENT_BIN="/usr/bin/gpclient"

# Redirect the output to a file for debugging then output to stdout
LOGFILE="/tmp/gpclient-hipreport.log"

HIP_REPORT_OUTPUT=$(exec 2> "$LOGFILE" "$GPCLIENT_BIN" hip -vv "$@")

echo "$HIP_REPORT_OUTPUT"
