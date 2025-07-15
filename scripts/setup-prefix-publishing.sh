#!/bin/bash

# Setup script for prefix.dev publishing
# This script helps you configure publishing to prefix.dev meso-forge channel

set -e

echo "🚀 Setting up prefix.dev publishing for GlobalProtect OpenConnect CLI"
echo "=================================================================="
echo

# Check if we're in the right directory
if [ ! -f "pixi.toml" ]; then
    echo "❌ Error: This script must be run from the project root directory"
    echo "   Please run this from the directory containing pixi.toml"
    exit 1
fi

# Check if rattler-build is available
if ! command -v rattler-build &> /dev/null; then
    echo "❌ Error: rattler-build is not available"
    echo "   Make sure you're in the pixi environment: pixi shell"
    exit 1
fi

echo "✅ Environment check passed"
echo

# Check for rattler auth file
if [ -n "$RATTLER_AUTH_FILE" ]; then
    echo "✅ RATTLER_AUTH_FILE is set: $RATTLER_AUTH_FILE"

    if [ -f "$RATTLER_AUTH_FILE" ]; then
        echo "✅ Auth file exists and is accessible"
        echo "   File size: $(du -h "$RATTLER_AUTH_FILE" | cut -f1)"
        echo "   Modified: $(stat -c %y "$RATTLER_AUTH_FILE" 2>/dev/null | cut -d' ' -f1 || date)"
    else
        echo "❌ Auth file not found at: $RATTLER_AUTH_FILE"
        echo
        echo "📋 To set up rattler authentication:"
        echo "   1. Go to https://prefix.dev/settings/api-keys"
        echo "   2. Create a new API key (starts with 'pfx_')"
        echo "   3. Create auth file with:"
        echo "   mkdir -p \$(dirname \$RATTLER_AUTH_FILE)"
        echo "   cat > \$RATTLER_AUTH_FILE << 'EOF'"
        echo "   {"
        echo "     \"*.prefix.dev\": {"
        echo "       \"BearerToken\": \"pfx_your_actual_key_here\""
        echo "     }"
        echo "   }"
        echo "   EOF"
        exit 1
    fi
else
    echo "❌ RATTLER_AUTH_FILE environment variable not set"
    echo "   This should be automatically set in the pixi environment"
    echo "   Current value: ${RATTLER_AUTH_FILE:-<unset>}"
    exit 1
fi

echo
echo "🔍 Checking for existing packages..."

# Check if package exists
if ls output/linux-64/globalprotect-openconnect-cli-*.conda >/dev/null 2>&1; then
    PACKAGE_FILE=$(ls output/linux-64/globalprotect-openconnect-cli-*.conda | head -n 1)
    echo "✅ Found package: $(basename "$PACKAGE_FILE")"

    # Get package info
    PACKAGE_SIZE=$(du -h "$PACKAGE_FILE" | cut -f1)
    echo "   Size: $PACKAGE_SIZE"
    echo "   Path: $PACKAGE_FILE"
else
    echo "⚠️  No CLI package found in output/linux-64/"
    echo "   Building package now..."
    echo

    if pixi run ship-cli; then
        echo "✅ Package built successfully!"
        PACKAGE_FILE=$(ls output/linux-64/globalprotect-openconnect-cli-*.conda | head -n 1)
        echo "   Package: $(basename "$PACKAGE_FILE")"
    else
        echo "❌ Failed to build package"
        exit 1
    fi
fi

echo
echo "🌐 Checking prefix.dev connection..."

# Test connection to prefix.dev
if curl -s --connect-timeout 5 https://prefix.dev > /dev/null; then
    echo "✅ Connection to prefix.dev successful"
else
    echo "❌ Cannot connect to prefix.dev. Check your internet connection."
    exit 1
fi

echo
echo "📦 Ready to publish!"
echo "=================="
echo
echo "Your package: $(basename "$PACKAGE_FILE")"
echo "Target channel: meso-forge"
echo "Destination: https://prefix.dev/channels/meso-forge"
echo
echo "🚀 Available publishing commands:"
echo "  pixi run publish-cli          - Upload the existing package"
echo "  pixi run publish-ship-cli     - Build and upload in one command"
echo
echo "💡 After publishing, your package will be available at:"
echo "   https://prefix.dev/channels/meso-forge/packages/globalprotect-openconnect-cli"
echo
echo "📖 To use the published package:"
echo "   pixi add --channel https://repo.prefix.dev/meso-forge globalprotect-openconnect-cli"
echo "   # or"
echo "   conda install -c https://repo.prefix.dev/meso-forge globalprotect-openconnect-cli"
echo
echo "✨ Setup complete! You're ready to publish to prefix.dev"

# Offer to publish now
echo
read -p "Would you like to publish the package now? (y/n): " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo
    echo "🚀 Publishing package..."
    if pixi run publish-cli; then
        echo
        echo "🎉 Package published successfully!"
        echo "   View it at: https://prefix.dev/channels/meso-forge"
    else
        echo
        echo "❌ Publishing failed. Check the error messages above."
        exit 1
    fi
else
    echo
    echo "📝 To publish later, run: pixi run publish-cli"
fi

echo
echo "Done! 🎉"
