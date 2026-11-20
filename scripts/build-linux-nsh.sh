#!/usr/bin/env bash
# Run in the gpgui checkout with its matching gp sibling on a native Linux builder.
set -euo pipefail

manifest=apps/desktop-nsh/Cargo.toml
toolchain=1.97.0
host=$(rustc "+$toolchain" -vV | sed -n 's/^host: //p')
case "$host" in
    x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) suffix= ;;
    x86_64-unknown-linux-musl|aarch64-unknown-linux-musl)
        suffix=-musl
        export RUSTFLAGS="-C target-feature=-crt-static"
        ;;
    *) echo "Unsupported Linux NSH build target: $host" >&2; exit 1 ;;
esac

pkg-config --exists 'gtk4 >= 4.14' gnutls dbus-1 wayland-client xkbcommon
cargo "+$toolchain" build --manifest-path "$manifest" --locked --release --bin gpgui-nsh \
    --target "$host" --target-dir target
binary="target/$host/release/gpgui-nsh"
"$binary" --version
needed=$(readelf -d "$binary")
if grep -q 'NEEDED.*libicu' <<<"$needed"; then
    echo 'The release executable must not depend on ICU4C.' >&2
    exit 1
fi
if [[ $suffix == -musl ]]; then
    readelf -lW "$binary" | grep 'Requesting program interpreter: /lib/ld-musl-' >/dev/null
fi
version=$(cargo "+$toolchain" metadata --manifest-path "$manifest" --no-deps --format-version 1 \
    | jq -er '.packages[] | select(.name == "gpgui-nsh") | .version')
directory="gpgui-nsh_$version"
archive="gpgui-nsh_$(uname -m)$suffix.bin.tar.xz"
mkdir -p ".build/package/$directory"
cp "$binary" apps/desktop-nsh/ICU4X-LICENSE.txt ".build/package/$directory/"
cd .build/package
tar -cJf "$archive" "$directory"
sha256sum "$archive" | cut -d ' ' -f 1 > "$archive.sha256"
