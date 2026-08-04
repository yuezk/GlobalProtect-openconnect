#!/bin/bash
#
# Build (and optionally install) GPCallback.app, the macOS handler for the
# `globalprotectcallback:` URL scheme used by external-browser authentication.
#
#   ./build-gpcallback.sh              # build into ./build
#   ./build-gpcallback.sh --install    # build, install to ~/Applications, register
#   ./build-gpcallback.sh --uninstall  # remove it again
#
# Only tools shipped with macOS are used: osacompile, PlistBuddy, codesign,
# lsregister. There is nothing to compile and no Rust involvement -- the applet
# just forwards the callback URL to `gpclient launch-gui`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE="$SCRIPT_DIR/gpcallback.applescript"
BUILD_DIR="${BUILD_DIR:-$SCRIPT_DIR/build}"
APP_NAME="GPCallback.app"
APP="$BUILD_DIR/$APP_NAME"
INSTALL_DIR="${INSTALL_DIR:-$HOME/Applications}"
BUNDLE_ID="com.yuezk.gpcallback"

PLIST_BUDDY=/usr/libexec/PlistBuddy
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: this script only applies to macOS" >&2
  exit 1
fi

uninstall() {
  if [ -d "$INSTALL_DIR/$APP_NAME" ]; then
    rm -rf "${INSTALL_DIR:?}/$APP_NAME"
    echo "removed $INSTALL_DIR/$APP_NAME"
  else
    echo "nothing to remove at $INSTALL_DIR/$APP_NAME"
  fi
  "$LSREGISTER" -kill -r -domain local -domain user >/dev/null 2>&1 || true
  echo "LaunchServices database rebuilt"
}

build() {
  rm -rf "$APP"
  mkdir -p "$BUILD_DIR"
  osacompile -o "$APP" "$SOURCE"

  local plist="$APP/Contents/Info.plist"

  # Claim the URL scheme. osacompile does not emit CFBundleURLTypes.
  "$PLIST_BUDDY" -c "Add :CFBundleIdentifier string $BUNDLE_ID" "$plist" 2>/dev/null \
    || "$PLIST_BUDDY" -c "Set :CFBundleIdentifier $BUNDLE_ID" "$plist"
  "$PLIST_BUDDY" -c "Add :CFBundleName string GPCallback" "$plist" 2>/dev/null || true
  # Background-only: no Dock icon, no menu bar takeover during login.
  "$PLIST_BUDDY" -c "Add :LSUIElement bool true" "$plist" 2>/dev/null || true
  "$PLIST_BUDDY" -c "Add :CFBundleURLTypes array" "$plist"
  "$PLIST_BUDDY" -c "Add :CFBundleURLTypes:0:CFBundleURLName string GlobalProtect Callback" "$plist"
  "$PLIST_BUDDY" -c "Add :CFBundleURLTypes:0:CFBundleTypeRole string Viewer" "$plist"
  "$PLIST_BUDDY" -c "Add :CFBundleURLTypes:0:CFBundleURLSchemes array" "$plist"
  "$PLIST_BUDDY" -c "Add :CFBundleURLTypes:0:CFBundleURLSchemes:0 string globalprotectcallback" "$plist"

  # Editing Info.plist invalidates the signature osacompile applied.
  codesign --force --deep --sign - "$APP"

  echo "built $APP"
}

install_app() {
  mkdir -p "$INSTALL_DIR"
  rm -rf "${INSTALL_DIR:?}/$APP_NAME"
  cp -R "$APP" "$INSTALL_DIR/"

  # Drop the staging copy before registering the installed one. LaunchServices
  # picks up bundles on sight, and two apps claiming globalprotectcallback:
  # makes which one receives the callback non-deterministic.
  "$LSREGISTER" -u "$APP" >/dev/null 2>&1 || true
  rm -rf "$APP"

  "$LSREGISTER" -f "$INSTALL_DIR/$APP_NAME"
  echo "installed $INSTALL_DIR/$APP_NAME and registered the globalprotectcallback: scheme"
  echo
  echo "verify with:"
  echo "  $LSREGISTER -dump | grep globalprotectcallback"
}

case "${1:-}" in
  --uninstall) uninstall ;;
  --install) build; install_app ;;
  "") build ;;
  *) echo "usage: $(basename "$0") [--install|--uninstall]" >&2; exit 1 ;;
esac
