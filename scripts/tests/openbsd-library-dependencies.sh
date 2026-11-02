#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT HUP INT TERM
mkdir -p "$temp/bin" "$temp/stage/bin"
touch "$temp/stage/bin/gpclient" "$temp/stage/bin/gpauth" "$temp/stage/bin/script"
cat > "$temp/bin/file" <<'EOF'
#!/bin/sh
case $2 in
    */script) echo 'POSIX shell script, ASCII text executable' ;;
    *) echo 'ELF 64-bit LSB shared object, x86-64' ;;
esac
EOF
cat > "$temp/bin/objdump" <<'EOF'
#!/bin/sh
case $2 in
    */gpclient)
        printf 'Dynamic Section:\n  NEEDED       libhogweed.so.3.1\n  NEEDED       libc.so.103.0\n'
        ;;
    */gpauth)
        printf '  NEEDED       libc.so.103.0\n  NEEDED       libc++abi.so.9.0\n'
        ;;
    *) exit 1 ;;
esac
EOF
chmod +x "$temp/bin/file" "$temp/bin/objdump"
PATH="$temp/bin:$PATH" sh "$root/scripts/openbsd-library-dependencies.sh" "$temp/stage" > "$temp/actual"
cat > "$temp/expected" <<'EOF'
@wantlib c++abi.9.0
@wantlib c.103.0
@wantlib hogweed.3.1
EOF
diff -u "$temp/expected" "$temp/actual"
# Failure to inspect an ELF file must stop packaging, not omit its requirements.
printf '#!/bin/sh\nexit 1\n' > "$temp/bin/objdump"
if PATH="$temp/bin:$PATH" sh "$root/scripts/openbsd-library-dependencies.sh" "$temp/stage" > /dev/null; then
    echo 'Expected ELF inspection failure' >&2
    exit 1
fi
echo 'OpenBSD library dependency checks passed.'
