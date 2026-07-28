#!/bin/bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <gpgui-repository>" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GP_DIR="$(dirname "$SCRIPT_DIR")"
GPGUI_DIR="$1"

verify_same_file() {
  local source_file="$1"
  local packaged_file="$2"

  if ! cmp --silent "$source_file" "$packaged_file"; then
    echo "Linux desktop asset is out of sync: $packaged_file" >&2
    echo "Expected it to match: $source_file" >&2
    exit 1
  fi
}

verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/32x32.png" \
  "$GP_DIR/packaging/files/usr/share/icons/hicolor/32x32/apps/gpgui.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/128x128.png" \
  "$GP_DIR/packaging/files/usr/share/icons/hicolor/128x128/apps/gpgui.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/128x128@2x.png" \
  "$GP_DIR/packaging/files/usr/share/icons/hicolor/256x256/apps/gpgui.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/icon.png" \
  "$GP_DIR/packaging/files/usr/share/icons/hicolor/256x256@2/apps/gpgui.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/icon.svg" \
  "$GP_DIR/packaging/files/usr/share/icons/hicolor/scalable/apps/gpgui.svg"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/32x32.png" \
  "$GP_DIR/apps/gpauth/icons/32x32.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/128x128.png" \
  "$GP_DIR/apps/gpauth/icons/128x128.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/128x128@2x.png" \
  "$GP_DIR/apps/gpauth/icons/128x128@2x.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/icon.png" \
  "$GP_DIR/apps/gpauth/icons/icon.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/32x32.png" \
  "$GP_DIR/apps/gpgui-helper/src-tauri/icons/32x32.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/128x128.png" \
  "$GP_DIR/apps/gpgui-helper/src-tauri/icons/128x128.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/128x128@2x.png" \
  "$GP_DIR/apps/gpgui-helper/src-tauri/icons/128x128@2x.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/icon.png" \
  "$GP_DIR/apps/gpgui-helper/src-tauri/icons/icon.png"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/icon.svg" \
  "$GP_DIR/apps/gpgui-helper/src-tauri/icons/icon.svg"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/icon.icns" \
  "$GP_DIR/apps/gpgui-helper/src-tauri/icons/icon.icns"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/icon.ico" \
  "$GP_DIR/apps/gpgui-helper/src-tauri/icons/icon.ico"
verify_same_file \
  "$GPGUI_DIR/app/src-tauri/icons/icon.svg" \
  "$GP_DIR/apps/gpgui-helper/src/assets/icon.svg"
verify_same_file \
  "$GPGUI_DIR/app/gpauth.desktop" \
  "$GP_DIR/packaging/files/usr/share/applications/gpauth.desktop"

echo "Linux desktop assets are synchronized"
