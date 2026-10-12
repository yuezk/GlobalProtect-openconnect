#!/bin/sh
# Resolve the library used by this build, rather than choosing a stale Cargo output.
set -eu
binary=$1
library=$(otool -L "$binary" | awk '/^[[:space:]]+@rpath\/libopenconnect\..*\.dylib / { sub("@rpath/", "", $1); print $1 }')
test -n "$library" || { echo "No OpenConnect dependency in $binary" >&2; exit 1; }
rpaths=$(otool -l "$binary" | awk '/cmd LC_RPATH/ { rpath=1; next } rpath && $1 == "path" { sub(/^[[:space:]]*path /, ""); sub(/ \(offset [0-9]+\)$/, ""); print; rpath=0 }')
while IFS= read -r directory; do
    case "$directory" in /*) ;; *) continue ;; esac
    if [ -f "$directory/$library" ]; then
        printf '%s\n' "$directory/$library"
        exit 0
    fi
done <<EOF
$rpaths
EOF
echo "Cannot resolve the OpenConnect build library for $binary" >&2
exit 1
