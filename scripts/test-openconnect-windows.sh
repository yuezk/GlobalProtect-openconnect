#!/usr/bin/env bash
# Run the native packet-pipe regression against the bundled Windows DLL.
set -euo pipefail
export PATH="/mingw64/bin:/usr/bin:$PATH"
repo=$(cygpath -u "$1")
prefix=$(cygpath -u "$2")
build_dir="$repo/target/openconnect-windows"
source_dir="$repo/crates/openconnect/deps/openconnect"
export PATH="$prefix/bin:$PATH"
gcc -Wall -Wextra -I"$build_dir" -I"$source_dir" -I"$source_dir/json" \
  $(pkg-config --cflags gnutls libxml-2.0) \
  "$repo/crates/openconnect/tests/windows_packet_pipe.c" \
  -L"$prefix/lib" -lopenconnect -lws2_32 -o "$build_dir/test-windows-packet-pipe.exe"
"$build_dir/test-windows-packet-pipe.exe"
