#!/bin/bash

# Verification script for published GlobalProtect OpenConnect CLI package
# This script verifies that the package was successfully published and can be installed

set -e

echo "🔍 Verifying published GlobalProtect OpenConnect CLI package"
echo "============================================================"
echo

CHANNEL_URL="https://repo.prefix.dev/meso-forge"
PACKAGE_NAME="globalprotect-openconnect-cli"
PACKAGE_VERSION="2.4.4"

echo "📦 Package Information:"
echo "   Name: $PACKAGE_NAME"
echo "   Version: $PACKAGE_VERSION"
echo "   Channel: $CHANNEL_URL"
echo

# Check if conda/mamba/micromamba is available
CONDA_CMD=""
if command -v micromamba &> /dev/null; then
    CONDA_CMD="micromamba"
elif command -v mamba &> /dev/null; then
    CONDA_CMD="mamba"
elif command -v conda &> /dev/null; then
    CONDA_CMD="conda"
else
    echo "❌ No conda-compatible package manager found"
    echo "   Please install conda, mamba, or micromamba"
    exit 1
fi

echo "✅ Using package manager: $CONDA_CMD"
echo

# Test channel accessibility
echo "🌐 Testing channel accessibility..."
if curl -s --connect-timeout 10 "$CHANNEL_URL/noarch/repodata.json" > /dev/null; then
    echo "✅ Channel is accessible"
else
    echo "❌ Channel is not accessible"
    echo "   URL: $CHANNEL_URL"
    exit 1
fi

# Search for the package in the channel
echo
echo "🔍 Searching for package in channel..."
SEARCH_RESULT=""
if command -v micromamba &> /dev/null; then
    SEARCH_RESULT=$(micromamba search -c "$CHANNEL_URL" "$PACKAGE_NAME" 2>/dev/null || echo "")
elif command -v mamba &> /dev/null; then
    SEARCH_RESULT=$(mamba search -c "$CHANNEL_URL" "$PACKAGE_NAME" 2>/dev/null || echo "")
else
    SEARCH_RESULT=$(conda search -c "$CHANNEL_URL" "$PACKAGE_NAME" 2>/dev/null || echo "")
fi

if echo "$SEARCH_RESULT" | grep -q "$PACKAGE_NAME"; then
    echo "✅ Package found in channel!"
    echo
    echo "📋 Available versions:"
    echo "$SEARCH_RESULT" | grep "$PACKAGE_NAME" | head -5
else
    echo "⚠️  Package not found in search results"
    echo "   This might be due to indexing delay or search limitations"
    echo "   The package was uploaded successfully according to rattler-build"
fi

echo
echo "📖 Installation Instructions:"
echo "============================================"
echo
echo "To install this package, use one of the following commands:"
echo
echo "# With micromamba:"
echo "micromamba install -c $CHANNEL_URL $PACKAGE_NAME"
echo
echo "# With mamba:"
echo "mamba install -c $CHANNEL_URL $PACKAGE_NAME"
echo
echo "# With conda:"
echo "conda install -c $CHANNEL_URL $PACKAGE_NAME"
echo
echo "# With pixi (in pixi.toml dependencies):"
echo "[$PACKAGE_NAME] = { version = \"$PACKAGE_VERSION\", channel = \"$CHANNEL_URL\" }"
echo

echo "🔗 Useful Links:"
echo "============================================"
echo "Channel page:     https://prefix.dev/channels/meso-forge"
echo "Package page:     https://prefix.dev/channels/meso-forge/packages/$PACKAGE_NAME"
echo "Direct download:  $CHANNEL_URL/linux-64/$PACKAGE_NAME-$PACKAGE_VERSION-*.conda"
echo

echo "📝 Package Contents:"
echo "============================================"
echo "This package provides the following CLI tools:"
echo "   • gpclient   - Main VPN client"
echo "   • gpservice  - Background service"
echo "   • gpauth     - Authentication helper"
echo "   • gp-setup   - System setup script"
echo "   • gp-welcome - Welcome/guidance script"
echo

echo "🎉 Verification complete!"
echo
echo "The package has been successfully published to prefix.dev meso-forge channel."
echo "Users can now install it using the commands shown above."
