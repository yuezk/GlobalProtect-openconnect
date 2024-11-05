#!/usr/bin/env bash

set -e

# Usage: ./deb-install.sh <version>

usage() {
  echo "Usage: $0 <version>"
  echo "Example: $0 2.3.9"
  exit 1
}

if [ $# -ne 1 ]; then
  usage
fi

VERSION=$1

# Check the architecture, only support x86_64 and aarch64/arm64
ARCH=$(uname -m)

# Normalize the architecture name
if [ "$ARCH" == "x86_64" ]; then
  ARCH="amd64"
elif [ "$ARCH" == "aarch64" ] || [ "$ARCH" == "arm64" ]; then
  ARCH="arm64"
else
  echo "Unsupported architecture: $ARCH"
  exit 1
fi

LIB_JAVASCRIPT_x86="http://launchpadlibrarian.net/704701345/libjavascriptcoregtk-4.0-18_2.43.3-1_amd64.deb"
LIB_WEBKIT_x86="http://launchpadlibrarian.net/704701349/libwebkit2gtk-4.0-37_2.43.3-1_amd64.deb"

LIB_JAVASCRIPT_arm="http://launchpadlibrarian.net/704735771/libjavascriptcoregtk-4.0-18_2.43.3-1_arm64.deb"
LIB_WEBKIT_arm="http://launchpadlibrarian.net/704735777/libwebkit2gtk-4.0-37_2.43.3-1_arm64.deb"

DEB_URL="https://github.com/yuezk/GlobalProtect-openconnect/releases/download/v${VERSION}/globalprotect-openconnect_${VERSION}-1_${ARCH}.deb"

# Install the dependencies
if [ "$ARCH" == "amd64" ]; then
  wget -O /tmp/libjavascriptcoregtk.deb $LIB_JAVASCRIPT_x86
  wget -O /tmp/libwebkit2gtk.deb $LIB_WEBKIT_x86
else
  wget -O /tmp/libjavascriptcoregtk.deb $LIB_JAVASCRIPT_arm
  wget -O /tmp/libwebkit2gtk.deb $LIB_WEBKIT_arm
fi

sudo dpkg -i /tmp/libjavascriptcoregtk.deb /tmp/libwebkit2gtk.deb

# Install the package
wget -O /tmp/globalprotect-openconnect.deb $DEB_URL
sudo apt install --fix-broken -y /tmp/globalprotect-openconnect.deb

# Clean up
rm /tmp/libjavascriptcoregtk.deb /tmp/libwebkit2gtk.deb /tmp/globalprotect-openconnect.deb

echo ""
echo "GlobalProtect OpenConnect VPN client has been installed successfully."
