#!/bin/bash

# GlobalProtect OpenConnect - PKG Config Environment Setup
# This script provides consistent PKG_CONFIG_PATH configuration across all tasks

set -euo pipefail

# Function to setup PKG_CONFIG_PATH with all necessary paths
setup_pkgconfig_path() {
    # Base conda paths
    local conda_lib="$CONDA_PREFIX/lib/pkgconfig"
    local conda_share="$CONDA_PREFIX/share/pkgconfig"

    # System paths for different distributions
    local system_paths=(
        "/usr/lib64/pkgconfig"
        "/usr/lib/x86_64-linux-gnu/pkgconfig"
        "/usr/lib/pkgconfig"
        "/usr/share/pkgconfig"
        "/usr/local/lib/pkgconfig"
        "/usr/local/share/pkgconfig"
    )

    # Build the PKG_CONFIG_PATH
    local pkg_config_path="$conda_lib:$conda_share"

    # Add system paths that exist
    for path in "${system_paths[@]}"; do
        if [[ -d "$path" ]]; then
            pkg_config_path="$pkg_config_path:$path"
        fi
    done

    export PKG_CONFIG_PATH="$pkg_config_path"

    # Verify CONDA_PREFIX is set
    if [[ -z "${CONDA_PREFIX:-}" ]]; then
        echo "ERROR: CONDA_PREFIX is not set. This script must be run within a pixi environment."
        exit 1
    fi

    # Verify conda pkg-config directories exist
    if [[ ! -d "$conda_lib" ]]; then
        echo "WARNING: Conda lib pkgconfig directory not found: $conda_lib"
    fi

    if [[ ! -d "$conda_share" ]]; then
        echo "WARNING: Conda share pkgconfig directory not found: $conda_share"
    fi
}

# Function to verify PKG_CONFIG_PATH is working
verify_pkgconfig() {
    local package="${1:-cairo}"

    if pkg-config --exists "$package" 2>/dev/null; then
        echo "✓ PKG_CONFIG: $package found"
        return 0
    else
        echo "✗ PKG_CONFIG: $package not found"
        return 1
    fi
}

# Function to show current PKG_CONFIG_PATH
show_pkgconfig_info() {
    echo "PKG_CONFIG_PATH setup:"
    echo "  CONDA_PREFIX: ${CONDA_PREFIX:-<not set>}"
    echo "  PKG_CONFIG_PATH: ${PKG_CONFIG_PATH:-<not set>}"
    echo ""

    if [[ -n "${PKG_CONFIG_PATH:-}" ]]; then
        echo "PKG_CONFIG_PATH components:"
        IFS=':' read -ra PATHS <<< "$PKG_CONFIG_PATH"
        for path in "${PATHS[@]}"; do
            if [[ -d "$path" ]]; then
                echo "  ✓ $path"
            else
                echo "  ✗ $path (missing)"
            fi
        done
    fi
}

# Function to test critical packages
test_critical_packages() {
    local packages=(
        "cairo"
        "gdk-3.0"
        "pango"
        "gdk-pixbuf-2.0"
        "openconnect"
    )

    echo "Testing critical packages:"
    local failed=0

    for package in "${packages[@]}"; do
        if verify_pkgconfig "$package"; then
            continue
        else
            failed=$((failed + 1))
        fi
    done

    if [[ $failed -eq 0 ]]; then
        echo "✓ All critical packages found"
        return 0
    else
        echo "✗ $failed critical packages missing"
        return 1
    fi
}

# Main function
main() {
    case "${1:-setup}" in
        "setup")
            setup_pkgconfig_path
            ;;
        "verify")
            setup_pkgconfig_path
            verify_pkgconfig "${2:-cairo}"
            ;;
        "info"|"show")
            setup_pkgconfig_path
            show_pkgconfig_info
            ;;
        "test")
            setup_pkgconfig_path
            test_critical_packages
            ;;
        "help"|"--help"|"-h")
            echo "Usage: $0 [setup|verify [package]|info|test|help]"
            echo ""
            echo "Commands:"
            echo "  setup           - Setup PKG_CONFIG_PATH environment (default)"
            echo "  verify [pkg]    - Verify package is found (default: cairo)"
            echo "  info|show       - Show PKG_CONFIG_PATH information"
            echo "  test            - Test all critical packages"
            echo "  help            - Show this help"
            echo ""
            echo "Examples:"
            echo "  $0 setup                    # Setup environment"
            echo "  $0 verify cairo             # Check if cairo is found"
            echo "  $0 test                     # Test all critical packages"
            echo "  source $0 setup             # Source to set in current shell"
            ;;
        *)
            echo "Unknown command: $1"
            echo "Use '$0 help' for usage information"
            exit 1
            ;;
    esac
}

# Only run main if script is executed (not sourced)
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
