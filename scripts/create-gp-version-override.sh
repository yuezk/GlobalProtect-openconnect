#!/bin/bash

# GlobalProtect Version Override - Elegant LD_PRELOAD Solution
# This script creates a dynamic library to override the csd_ticket behavior
# without patching OpenConnect source code

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "=== Creating Elegant GlobalProtect Version Override ==="
echo ""
echo "This solution uses LD_PRELOAD to intercept the version reporting"
echo "without modifying OpenConnect source code at all!"
echo ""

cd "$PROJECT_ROOT"

# Create build directory for our override
mkdir -p build/gp-override

cat > build/gp-override/gp_version_override.c << 'EOF'
/*
 * GlobalProtect Version Override Library
 *
 * This library uses LD_PRELOAD to intercept OpenConnect's version reporting
 * and allows overriding the GlobalProtect app version via environment variable.
 *
 * Usage:
 *   export GP_APP_VERSION=6.3.0
 *   LD_PRELOAD=./gp_version_override.so openconnect --protocol=gp server.com
 *
 * Or use the wrapper script which does this automatically.
 */

#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Function pointer for the original append_opt */
static void (*original_append_opt)(void *buf, const char *name, const char *val) = NULL;

/* Override function that intercepts append_opt calls */
void append_opt(void *buf, const char *name, const char *val) {
    /* Load the original function if not already loaded */
    if (!original_append_opt) {
        original_append_opt = dlsym(RTLD_NEXT, "append_opt");
        if (!original_append_opt) {
            fprintf(stderr, "Error: Could not find original append_opt function\n");
            exit(1);
        }
    }

    /* Check if this is an app-version parameter and we have an override */
    if (name && strcmp(name, "app-version") == 0) {
        const char *override_version = getenv("GP_APP_VERSION");
        if (override_version && strlen(override_version) > 0) {
            fprintf(stderr, "GP Override: Using app-version %s (was: %s)\n",
                    override_version, val ? val : "NULL");
            original_append_opt(buf, name, override_version);
            return;
        }
    }

    /* For all other parameters, or when no override is set, use original */
    original_append_opt(buf, name, val);
}

/* Constructor to announce the override is active */
__attribute__((constructor))
void gp_override_init() {
    const char *override_version = getenv("GP_APP_VERSION");
    if (override_version) {
        fprintf(stderr, "GP Override: Active - will report version %s\n", override_version);
    } else {
        fprintf(stderr, "GP Override: Loaded but no GP_APP_VERSION set - using default behavior\n");
    }
}
EOF

# Create a Makefile for the override library
cat > build/gp-override/Makefile << 'EOF'
# Makefile for GlobalProtect Version Override Library

CC = gcc
CFLAGS = -fPIC -shared -Wall -O2
LDFLAGS = -ldl

TARGET = gp_version_override.so
SOURCE = gp_version_override.c

all: $(TARGET)

$(TARGET): $(SOURCE)
	$(CC) $(CFLAGS) -o $(TARGET) $(SOURCE) $(LDFLAGS)

clean:
	rm -f $(TARGET)

install: $(TARGET)
	cp $(TARGET) ../..

.PHONY: all clean install
EOF

# Build the override library
echo "Building GlobalProtect version override library..."
cd build/gp-override

if command -v gcc &> /dev/null; then
    make
    echo "✅ Override library built successfully"
else
    echo "❌ ERROR: gcc not found. Cannot build override library."
    echo "Please install gcc or use the patch-based approach instead."
    exit 1
fi

# Copy the library to the project root for easy access
cp gp_version_override.so "$PROJECT_ROOT/"

# Create a wrapper script that makes it easy to use
cat > "$PROJECT_ROOT/scripts/openconnect-gp" << 'EOF'
#!/bin/bash

# OpenConnect GlobalProtect Wrapper with Version Override
# This script allows easy overriding of the GlobalProtect app version

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
OVERRIDE_LIB="$PROJECT_ROOT/gp_version_override.so"

# Default to a modern, compatible version
DEFAULT_GP_VERSION="6.3.0"

# Show usage information
usage() {
    cat << 'USAGE_EOF'
OpenConnect GlobalProtect Wrapper with Version Override

Usage:
  openconnect-gp [options] <server>
  openconnect-gp --gp-version=VERSION [options] <server>

GlobalProtect Version Options:
  --gp-version=VERSION    Set GlobalProtect app version (default: 6.3.0)
                          Common values: 6.1.4, 6.2.0, 6.3.0, 6.3.3

Environment Variables:
  GP_APP_VERSION          Override the GlobalProtect version
                          (command line --gp-version takes precedence)

Examples:
  # Use default version (6.3.0)
  openconnect-gp vpn.company.com

  # Specify version on command line
  openconnect-gp --gp-version=6.1.4 vpn.company.com

  # Use environment variable
  GP_APP_VERSION=6.2.0 openconnect-gp vpn.company.com

  # Pass additional OpenConnect options
  openconnect-gp --gp-version=6.3.0 --user=john vpn.company.com

All other options are passed directly to openconnect.
USAGE_EOF
}

# Check if override library exists
if [ ! -f "$OVERRIDE_LIB" ]; then
    echo "ERROR: Override library not found at $OVERRIDE_LIB"
    echo "Please run: pixi run create-gp-version-override"
    exit 1
fi

# Parse command line arguments
GP_VERSION=""
OPENCONNECT_ARGS=()

while [[ $# -gt 0 ]]; do
    case $1 in
        --gp-version=*)
            GP_VERSION="${1#*=}"
            shift
            ;;
        --gp-version)
            GP_VERSION="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            OPENCONNECT_ARGS+=("$1")
            shift
            ;;
    esac
done

# Determine which version to use
if [ -n "$GP_VERSION" ]; then
    # Command line version takes precedence
    export GP_APP_VERSION="$GP_VERSION"
elif [ -z "$GP_APP_VERSION" ]; then
    # No version specified, use default
    export GP_APP_VERSION="$DEFAULT_GP_VERSION"
fi

echo "Using GlobalProtect app version: $GP_APP_VERSION"

# Add --protocol=gp if not already present
PROTOCOL_SET=false
for arg in "${OPENCONNECT_ARGS[@]}"; do
    if [[ "$arg" == "--protocol=gp" ]] || [[ "$arg" == "--protocol"* ]]; then
        PROTOCOL_SET=true
        break
    fi
done

if [ "$PROTOCOL_SET" = false ]; then
    OPENCONNECT_ARGS=("--protocol=gp" "${OPENCONNECT_ARGS[@]}")
fi

# Run openconnect with our override library
echo "Running: LD_PRELOAD=$OVERRIDE_LIB openconnect ${OPENCONNECT_ARGS[*]}"
echo ""

LD_PRELOAD="$OVERRIDE_LIB" openconnect "${OPENCONNECT_ARGS[@]}"
EOF

chmod +x "$PROJECT_ROOT/scripts/openconnect-gp"

echo ""
echo "✅ Elegant GlobalProtect Version Override Created!"
echo ""
echo "=== What Was Created ==="
echo ""
echo "1. Override library: gp_version_override.so"
echo "   - Intercepts version reporting using LD_PRELOAD"
echo "   - No OpenConnect source modifications needed"
echo ""
echo "2. Wrapper script: scripts/openconnect-gp"
echo "   - Easy-to-use interface for version override"
echo "   - Automatically sets up LD_PRELOAD"
echo ""
echo "=== Usage Examples ==="
echo ""
echo "# Use default version (6.3.0)"
echo "scripts/openconnect-gp your-server.com"
echo ""
echo "# Specify a different version"
echo "scripts/openconnect-gp --gp-version=6.1.4 your-server.com"
echo ""
echo "# Manual override with environment variable"
echo "GP_APP_VERSION=6.2.0 LD_PRELOAD=./gp_version_override.so openconnect --protocol=gp your-server.com"
echo ""
echo "=== Benefits of This Approach ==="
echo ""
echo "✅ No source code patches needed"
echo "✅ Works with any OpenConnect version"
echo "✅ User can specify any version"
echo "✅ Easy to enable/disable"
echo "✅ No compilation of OpenConnect required"
echo "✅ Backward compatible"
echo ""
echo "This is the most elegant solution - it requires no changes to OpenConnect itself!"
echo ""
echo "To test: scripts/openconnect-gp --gp-version=6.3.0 your-vpn-server.com"
