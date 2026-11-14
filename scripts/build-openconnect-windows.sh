#!/usr/bin/env bash
# Build the bundled protocol library with MSYS2's native Windows compiler.
set -euo pipefail
export PATH="/mingw64/bin:/usr/bin:$PATH"
repo=$(cygpath -u "$1")
prefix=$(cygpath -u "$2")
jobs=$3
export ACLOCAL_PATH=/mingw64/share/aclocal
source_dir="$repo/crates/openconnect/deps/openconnect"
build_dir="$repo/target/openconnect-windows"
cd "$source_dir"
autoreconf -fi
mkdir -p "$build_dir"
cd "$build_dir"
"$source_dir/configure" --host=x86_64-w64-mingw32 --prefix="$prefix" \
  --enable-shared --disable-static --disable-nls --disable-docs --disable-dsa-tests \
  --without-libproxy --without-stoken --without-libpcsclite --without-libpskc \
  --without-gssapi --with-gnutls --without-openssl --with-gnutls-tss2=no \
  --with-vpnc-script=unused
grep -q '^#define OPENCONNECT_GNUTLS 1$' config.h
make -j"$jobs" libopenconnect.la
make install-libLTLIBRARIES
install -Dm644 "$source_dir/openconnect.h" "$prefix/include/openconnect.h"
