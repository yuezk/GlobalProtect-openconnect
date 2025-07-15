#!/bin/bash

# Setup script for PKG_CONFIG_PATH in pixi environment
# This script ensures that the conda environment's pkg-config files are found

if [ -n "$CONDA_PREFIX" ]; then
    # Add conda environment's pkg-config directory to PKG_CONFIG_PATH
    export PKG_CONFIG_PATH="$CONDA_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

    # Also check for share/pkgconfig (some packages put files there)
    if [ -d "$CONDA_PREFIX/share/pkgconfig" ]; then
        export PKG_CONFIG_PATH="$CONDA_PREFIX/share/pkgconfig:$PKG_CONFIG_PATH"
    fi

    echo "PKG_CONFIG_PATH configured: $PKG_CONFIG_PATH"
else
    echo "Warning: CONDA_PREFIX not set, PKG_CONFIG_PATH may not be properly configured"
fi
