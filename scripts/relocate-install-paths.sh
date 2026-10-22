#!/bin/sh
# Integration files use the runtime prefix, never the packaging staging directory.
set -eu
prefix=$1
source_prefix=$2
directory=$3
escape_replacement() {
    printf '%s' "$1" | sed 's/[\\&|]/\\&/g'
}
# Each integration format has its own quoting rules.
desktop_prefix=$(printf '%s' "$prefix" | sed -e 's/\\/\\\\\\\\/g' -e 's/["`$]/\\\\&/g' -e 's/%/%%/g')
# Avoid unnecessary quotes for older xdg-open parsers.
case "$prefix/bin/gpclient" in
    *[!A-Za-z0-9_./-]*) desktop_command=$(escape_replacement "\"$desktop_prefix/bin/gpclient\"") ;;
    *) desktop_command=$(escape_replacement "$desktop_prefix/bin/gpclient") ;;
esac
shell_prefix=$(printf '%s' "$prefix" | sed "s/'/'\\\\''/g")
shell_command=$(escape_replacement "'$shell_prefix/bin/gpclient'")
xml_prefix=$(printf '%s' "$prefix" | sed -e 's/\&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g')
xml_replacement=$(escape_replacement "$xml_prefix")
temporary=$(mktemp)
trap 'rm -f "$temporary"' EXIT HUP INT TERM
for file in share/applications/gpgui.desktop \
    share/polkit-1/actions/com.yuezk.gpgui.policy \
    lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down \
    lib/NetworkManager/dispatcher.d/gpclient-nm-hook; do
    path="$directory/$file"
    [ -f "$path" ] || continue
    case "$file" in
        share/applications/*)
            sed "s|$source_prefix/bin/gpclient|$desktop_command|g" "$path" > "$temporary" ;;
        share/polkit-1/*)
            sed -e "s|$source_prefix/bin/|$xml_replacement/bin/|g" \
                -e "s|$source_prefix/libexec/gpclient/|$xml_replacement/libexec/gpclient/|g" \
                "$path" > "$temporary" ;;
        lib/*)
            sed "s|$source_prefix/bin/gpclient|$shell_command|g" "$path" > "$temporary" ;;
    esac
    mode=644
    case "$file" in lib/*) mode=755 ;; esac
    install -m "$mode" "$temporary" "$path"
done
