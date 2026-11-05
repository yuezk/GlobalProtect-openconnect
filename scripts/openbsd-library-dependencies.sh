#!/bin/sh
# Record the library ABIs required by the staged OpenBSD executables.
set -eu

root=$1
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT HUP INT TERM
find "$root" -type f > "$temp/files"
while IFS= read -r file; do
    case $(file -b "$file") in
        ELF*) objdump -p "$file" >> "$temp/headers" ;;
    esac
done < "$temp/files"
test -s "$temp/headers"
sed -n 's/^[[:space:]]*NEEDED[[:space:]]*lib\([^[:space:]]*\)\.so\.\([0-9][0-9]*\.[0-9][0-9]*\)[[:space:]]*$/@wantlib \1.\2/p' \
    "$temp/headers" | LC_ALL=C sort -u
