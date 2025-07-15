#!/bin/bash

# GlobalProtect OpenConnect Installation Verification Script
# This script verifies that the conda package is properly installed and configured

set -euo pipefail

VERSION="2.4.4"
SCRIPT_NAME="verify-installation"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Helper functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[✓]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[⚠]${NC} $1"
}

log_error() {
    echo -e "${RED}[✗]${NC} $1"
}

test_start() {
    TESTS_RUN=$((TESTS_RUN + 1))
    echo -e "${CYAN}[TEST $TESTS_RUN]${NC} $1"
}

test_pass() {
    TESTS_PASSED=$((TESTS_PASSED + 1))
    log_success "$1"
}

test_fail() {
    TESTS_FAILED=$((TESTS_FAILED + 1))
    log_error "$1"
}

print_header() {
    echo
    echo -e "${BOLD}${CYAN}================================================${NC}"
    echo -e "${BOLD}${CYAN} GlobalProtect OpenConnect Installation Verification${NC}"
    echo -e "${BOLD}${CYAN} Version: $VERSION${NC}"
    echo -e "${BOLD}${CYAN}================================================${NC}"
    echo
}

print_summary() {
    echo
    echo -e "${BOLD}${CYAN}================================================${NC}"
    echo -e "${BOLD}${CYAN} Verification Summary${NC}"
    echo -e "${BOLD}${CYAN}================================================${NC}"
    echo "Tests run:    $TESTS_RUN"
    echo "Tests passed: $TESTS_PASSED"
    echo "Tests failed: $TESTS_FAILED"
    echo

    if [[ $TESTS_FAILED -eq 0 ]]; then
        echo -e "${GREEN}${BOLD}✓ All verification tests passed!${NC}"
        echo -e "${GREEN}GlobalProtect OpenConnect is properly installed.${NC}"
        echo
        echo -e "${YELLOW}Next steps:${NC}"
        echo "1. Run: ${CYAN}gp-setup --all${NC}"
        echo "2. Test: ${CYAN}gpauth --browser default your-server.com${NC}"
        echo "3. Connect: ${CYAN}gpclient connect your-server.com${NC}"
        return 0
    else
        echo -e "${RED}${BOLD}✗ $TESTS_FAILED verification test(s) failed!${NC}"
        echo -e "${RED}Installation may be incomplete or corrupted.${NC}"
        echo
        echo -e "${YELLOW}Troubleshooting:${NC}"
        echo "• Reinstall: ${CYAN}conda install -c conda-forge globalprotect-openconnect${NC}"
        echo "• Check environment: ${CYAN}conda list globalprotect-openconnect${NC}"
        echo "• Verify PATH: ${CYAN}which gpclient${NC}"
        return 1
    fi
}

# Test if command exists and is executable
test_command() {
    local cmd="$1"
    local expected_version="$2"

    test_start "Checking $cmd command"

    if ! command -v "$cmd" >/dev/null 2>&1; then
        test_fail "$cmd not found in PATH"
        return 1
    fi

    local cmd_path
    cmd_path=$(which "$cmd")
    log_info "Found at: $cmd_path"

    if [[ ! -x "$cmd_path" ]]; then
        test_fail "$cmd is not executable"
        return 1
    fi

    # Test if command can show help
    if ! "$cmd" --help >/dev/null 2>&1; then
        test_fail "$cmd --help failed"
        return 1
    fi

    # Check version if possible
    if "$cmd" --version >/dev/null 2>&1; then
        local version_output
        version_output=$("$cmd" --version 2>&1 | head -1 || echo "unknown")
        log_info "Version: $version_output"
    fi

    test_pass "$cmd is properly installed and executable"
    return 0
}

# Test file existence and permissions
test_file() {
    local file="$1"
    local description="$2"
    local expected_perms="${3:-}"

    test_start "Checking $description"

    if [[ ! -f "$file" ]]; then
        test_fail "$description not found: $file"
        return 1
    fi

    if [[ -n "$expected_perms" ]]; then
        local actual_perms
        actual_perms=$(stat -c "%a" "$file" 2>/dev/null || echo "unknown")
        if [[ "$actual_perms" != "$expected_perms" ]]; then
            test_fail "$description has incorrect permissions: $actual_perms (expected: $expected_perms)"
            return 1
        fi
    fi

    test_pass "$description found and accessible"
    return 0
}

# Test directory existence and permissions
test_directory() {
    local dir="$1"
    local description="$2"
    local create_if_missing="${3:-false}"

    test_start "Checking $description"

    if [[ ! -d "$dir" ]]; then
        if [[ "$create_if_missing" == "true" ]]; then
            log_info "Creating missing directory: $dir"
            if mkdir -p "$dir" 2>/dev/null; then
                test_pass "$description created successfully"
                return 0
            else
                test_fail "Failed to create $description: $dir"
                return 1
            fi
        else
            test_fail "$description not found: $dir"
            return 1
        fi
    fi

    if [[ ! -r "$dir" ]]; then
        test_fail "$description is not readable: $dir"
        return 1
    fi

    test_pass "$description found and accessible"
    return 0
}

# Test conda environment
test_conda_environment() {
    test_start "Checking conda environment"

    if [[ -z "${CONDA_PREFIX:-}" ]]; then
        test_fail "CONDA_PREFIX not set - not running in conda environment"
        return 1
    fi

    log_info "Conda prefix: $CONDA_PREFIX"

    # Check if conda is available
    if ! command -v conda >/dev/null 2>&1; then
        test_warning "conda command not available, but CONDA_PREFIX is set"
    else
        local conda_info
        conda_info=$(conda info --envs 2>/dev/null | grep "$(basename "$CONDA_PREFIX")" || echo "")
        if [[ -n "$conda_info" ]]; then
            log_info "Active environment: $(echo "$conda_info" | awk '{print $1}')"
        fi
    fi

    test_pass "Conda environment properly configured"
    return 0
}

# Test desktop integration
test_desktop_integration() {
    test_start "Checking desktop integration"

    local desktop_file="$CONDA_PREFIX/share/applications/gpgui.desktop"
    if [[ -f "$desktop_file" ]]; then
        log_info "Desktop file found: $desktop_file"

        # Check if desktop file is valid
        if grep -q "GlobalProtect" "$desktop_file" 2>/dev/null; then
            test_pass "Desktop integration properly installed"
            return 0
        else
            test_fail "Desktop file appears corrupted"
            return 1
        fi
    else
        test_warning "Desktop file not found (optional component)"
        return 0
    fi
}

# Test icon files
test_icons() {
    test_start "Checking icon files"

    local icon_found=false
    local icon_dirs=(
        "$CONDA_PREFIX/share/icons/hicolor/scalable/apps"
        "$CONDA_PREFIX/share/icons/hicolor/32x32/apps"
        "$CONDA_PREFIX/share/icons/hicolor/128x128/apps"
        "$CONDA_PREFIX/share/icons/hicolor/256x256@2/apps"
    )

    for icon_dir in "${icon_dirs[@]}"; do
        if [[ -f "$icon_dir/gpgui.svg" ]] || [[ -f "$icon_dir/gpgui.png" ]]; then
            icon_found=true
            log_info "Icon found in: $icon_dir"
        fi
    done

    if [[ "$icon_found" == "true" ]]; then
        test_pass "Icon files properly installed"
    else
        test_warning "No icon files found (optional component)"
    fi

    return 0
}

# Test system dependencies
test_system_dependencies() {
    test_start "Checking system dependencies"

    local missing_deps=()
    local system_deps=("openssl" "pkexec")

    for dep in "${system_deps[@]}"; do
        if ! command -v "$dep" >/dev/null 2>&1; then
            missing_deps+=("$dep")
        fi
    done

    if [[ ${#missing_deps[@]} -eq 0 ]]; then
        test_pass "All system dependencies available"
    else
        test_warning "Missing optional system dependencies: ${missing_deps[*]}"
        log_info "Some features may not work without these dependencies"
    fi

    return 0
}

# Test documentation
test_documentation() {
    test_start "Checking documentation"

    local doc_dir="$CONDA_PREFIX/share/doc/globalprotect-openconnect"
    if [[ -d "$doc_dir" ]]; then
        local doc_files=()
        if [[ -f "$doc_dir/README-conda.md" ]]; then
            doc_files+=("README-conda.md")
        fi
        if [[ -f "$doc_dir/post-install-message.sh" ]]; then
            doc_files+=("post-install-message.sh")
        fi

        if [[ ${#doc_files[@]} -gt 0 ]]; then
            test_pass "Documentation files found: ${doc_files[*]}"
            log_info "Documentation directory: $doc_dir"
        else
            test_warning "Documentation directory exists but appears empty"
        fi
    else
        test_warning "Documentation directory not found (optional)"
    fi

    return 0
}

# Test basic functionality
test_basic_functionality() {
    test_start "Testing basic functionality"

    # Test gp-setup check
    if command -v gp-setup >/dev/null 2>&1; then
        if gp-setup --check >/dev/null 2>&1; then
            test_pass "gp-setup check functionality works"
        else
            test_fail "gp-setup check failed"
            return 1
        fi
    else
        test_fail "gp-setup not available for functionality test"
        return 1
    fi

    # Test gpauth help (lightweight test)
    if command -v gpauth >/dev/null 2>&1; then
        if gpauth --help >/dev/null 2>&1; then
            test_pass "gpauth help functionality works"
        else
            test_fail "gpauth help failed"
            return 1
        fi
    else
        test_fail "gpauth not available for functionality test"
        return 1
    fi

    return 0
}

# Main verification function
run_verification() {
    print_header

    # Test conda environment
    test_conda_environment

    # Test main binaries
    test_command "gpclient" "$VERSION"
    test_command "gpservice" "$VERSION"
    test_command "gpauth" "$VERSION"
    test_command "gpgui-helper" "$VERSION"
    test_command "gp-setup" "$VERSION"
    test_command "gp-welcome" "$VERSION"

    # Test desktop integration
    test_desktop_integration
    test_icons

    # Test documentation
    test_documentation

    # Test system dependencies
    test_system_dependencies

    # Test basic functionality
    test_basic_functionality

    # Print summary and exit with appropriate code
    print_summary
}

# Parse command line arguments
case "${1:-}" in
    --help|-h)
        echo "Usage: $SCRIPT_NAME [OPTIONS]"
        echo
        echo "Verify GlobalProtect OpenConnect conda package installation"
        echo
        echo "OPTIONS:"
        echo "  --help, -h    Show this help message"
        echo "  --quiet, -q   Run with minimal output"
        echo "  --verbose, -v Run with detailed output"
        echo
        echo "EXIT CODES:"
        echo "  0  All verification tests passed"
        echo "  1  One or more verification tests failed"
        echo
        exit 0
        ;;
    --quiet|-q)
        # Redirect output to reduce verbosity
        exec > >(grep -E "(\[✓\]|\[✗\]|\[TEST\]|Verification Summary|All verification|failed!)" || cat)
        ;;
    --verbose|-v)
        # Enable verbose output (default behavior)
        ;;
    "")
        # Default behavior
        ;;
    *)
        echo "Unknown option: $1"
        echo "Run '$SCRIPT_NAME --help' for usage information."
        exit 1
        ;;
esac

# Run the verification
run_verification
