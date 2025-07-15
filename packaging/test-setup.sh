#!/bin/bash

# Test script for gp-setup functionality
# This script validates that the gp-setup script works correctly

set -euo pipefail

TEST_DIR="/tmp/gp-setup-test-$$"
TEST_HOME="$TEST_DIR/home"
TEST_XDG_RUNTIME_DIR="$TEST_DIR/runtime"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
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
    echo -e "${GREEN}[PASS]${NC} $1"
}

log_error() {
    echo -e "${RED}[FAIL]${NC} $1"
}

test_start() {
    TESTS_RUN=$((TESTS_RUN + 1))
    echo -e "${YELLOW}[TEST $TESTS_RUN]${NC} $1"
}

test_pass() {
    TESTS_PASSED=$((TESTS_PASSED + 1))
    log_success "$1"
}

test_fail() {
    TESTS_FAILED=$((TESTS_FAILED + 1))
    log_error "$1"
}

# Setup test environment
setup_test_env() {
    log_info "Setting up test environment"

    # Create test directories
    mkdir -p "$TEST_HOME/.local/share/applications"
    mkdir -p "$TEST_HOME/.local/state"
    mkdir -p "$TEST_HOME/.config"
    mkdir -p "$TEST_XDG_RUNTIME_DIR"

    # Set environment variables
    export HOME="$TEST_HOME"
    export XDG_RUNTIME_DIR="$TEST_XDG_RUNTIME_DIR"
    export PATH="$(dirname "$(which gp-setup)"):$PATH"

    log_success "Test environment created: $TEST_DIR"
}

# Cleanup test environment
cleanup_test_env() {
    log_info "Cleaning up test environment"
    rm -rf "$TEST_DIR"
    log_success "Test environment cleaned up"
}

# Test gp-setup help
test_help() {
    test_start "Testing gp-setup --help"

    if gp-setup --help > /dev/null 2>&1; then
        test_pass "Help command works"
    else
        test_fail "Help command failed"
    fi
}

# Test gp-setup check
test_check() {
    test_start "Testing gp-setup --check"

    if gp-setup --check > /dev/null 2>&1; then
        test_pass "Check command works"
    else
        test_fail "Check command failed"
    fi
}

# Test URL handler setup
test_url_handler() {
    test_start "Testing URL handler setup"

    # Run URL handler setup
    if gp-setup --url-handler > /dev/null 2>&1; then
        # Check if desktop file was created
        local desktop_file="$TEST_HOME/.local/share/applications/gpclient-callback.desktop"
        if [[ -f "$desktop_file" ]]; then
            # Check desktop file content
            if grep -q "globalprotectcallback" "$desktop_file"; then
                test_pass "URL handler setup successful"
            else
                test_fail "Desktop file missing globalprotectcallback MIME type"
            fi
        else
            test_fail "Desktop file not created"
        fi
    else
        test_fail "URL handler setup failed"
    fi
}

# Test runtime directory setup
test_runtime_dirs() {
    test_start "Testing runtime directory setup"

    if gp-setup --runtime-dirs > /dev/null 2>&1; then
        local runtime_dir="$TEST_HOME/.local/state/globalprotect"
        if [[ -d "$runtime_dir" ]]; then
            # Check permissions
            local perms
            perms=$(stat -c "%a" "$runtime_dir")
            if [[ "$perms" == "700" ]]; then
                test_pass "Runtime directory setup successful"
            else
                test_fail "Runtime directory has incorrect permissions: $perms"
            fi
        else
            test_fail "Runtime directory not created"
        fi
    else
        test_fail "Runtime directory setup failed"
    fi
}

# Test permission fixing
test_permissions() {
    test_start "Testing permission fixing"

    # Create some test files with wrong permissions
    local test_dir="$TEST_HOME/.config/globalprotect"
    mkdir -p "$test_dir"
    touch "$test_dir/test.conf"
    chmod 777 "$test_dir"
    chmod 666 "$test_dir/test.conf"

    if gp-setup --permissions > /dev/null 2>&1; then
        # Check if permissions were fixed
        local dir_perms
        local file_perms
        dir_perms=$(stat -c "%a" "$test_dir")
        file_perms=$(stat -c "%a" "$test_dir/test.conf")

        if [[ "$dir_perms" == "700" ]] && [[ "$file_perms" == "600" ]]; then
            test_pass "Permission fixing successful"
        else
            test_fail "Permissions not fixed correctly (dir: $dir_perms, file: $file_perms)"
        fi
    else
        test_fail "Permission fixing failed"
    fi
}

# Test Flatpak configuration (mock)
test_flatpak_config() {
    test_start "Testing Flatpak configuration"

    # Mock flatpak command to avoid dependency
    local mock_flatpak="$TEST_DIR/flatpak"
    cat > "$mock_flatpak" << 'EOF'
#!/bin/bash
case "$1" in
    "list")
        echo "org.mozilla.firefox	Firefox	stable	system"
        ;;
    "override")
        exit 0
        ;;
    *)
        exit 1
        ;;
esac
EOF
    chmod +x "$mock_flatpak"
    export PATH="$TEST_DIR:$PATH"

    if gp-setup --flatpak > /dev/null 2>&1; then
        test_pass "Flatpak configuration completed"
    else
        test_fail "Flatpak configuration failed"
    fi
}

# Test all setup
test_all_setup() {
    test_start "Testing complete setup (--all)"

    # Clean up previous test artifacts
    rm -rf "$TEST_HOME/.local/share/applications/gpclient-callback.desktop"
    rm -rf "$TEST_HOME/.local/state/globalprotect"

    if gp-setup --all > /dev/null 2>&1; then
        # Check if all components were set up
        local checks=0
        local passed=0

        # Check URL handler
        checks=$((checks + 1))
        if [[ -f "$TEST_HOME/.local/share/applications/gpclient-callback.desktop" ]]; then
            passed=$((passed + 1))
        fi

        # Check runtime directory
        checks=$((checks + 1))
        if [[ -d "$TEST_HOME/.local/state/globalprotect" ]]; then
            passed=$((passed + 1))
        fi

        if [[ $passed -eq $checks ]]; then
            test_pass "Complete setup successful"
        else
            test_fail "Complete setup incomplete ($passed/$checks checks passed)"
        fi
    else
        test_fail "Complete setup failed"
    fi
}

# Test configuration check after setup
test_config_check_after_setup() {
    test_start "Testing configuration check after setup"

    local output
    output=$(gp-setup --check 2>&1 || true)

    # Check for expected elements in output
    if echo "$output" | grep -q "Binary Status" && \
       echo "$output" | grep -q "URL Scheme Handler" && \
       echo "$output" | grep -q "Runtime Directories"; then
        test_pass "Configuration check shows expected sections"
    else
        test_fail "Configuration check missing expected sections"
    fi
}

# Test uninstall
test_uninstall() {
    test_start "Testing uninstall"

    # First ensure something is installed
    gp-setup --url-handler > /dev/null 2>&1 || true

    # Test uninstall with automatic yes
    if echo "n" | gp-setup --uninstall > /dev/null 2>&1; then
        # Check if desktop file was removed
        if [[ ! -f "$TEST_HOME/.local/share/applications/gpclient-callback.desktop" ]]; then
            test_pass "Uninstall successful"
        else
            test_fail "Desktop file not removed during uninstall"
        fi
    else
        test_fail "Uninstall failed"
    fi
}

# Test error handling
test_error_handling() {
    test_start "Testing error handling with invalid options"

    if ! gp-setup --invalid-option > /dev/null 2>&1; then
        test_pass "Invalid option properly rejected"
    else
        test_fail "Invalid option not rejected"
    fi
}

# Main test runner
run_tests() {
    echo "=================================="
    echo "  GP-Setup Test Suite"
    echo "=================================="
    echo

    setup_test_env

    # Run all tests
    test_help
    test_check
    test_url_handler
    test_runtime_dirs
    test_permissions
    test_flatpak_config
    test_all_setup
    test_config_check_after_setup
    test_uninstall
    test_error_handling

    cleanup_test_env

    # Print results
    echo
    echo "=================================="
    echo "  Test Results"
    echo "=================================="
    echo "Tests run: $TESTS_RUN"
    echo "Tests passed: $TESTS_PASSED"
    echo "Tests failed: $TESTS_FAILED"

    if [[ $TESTS_FAILED -eq 0 ]]; then
        echo -e "${GREEN}All tests passed!${NC}"
        exit 0
    else
        echo -e "${RED}$TESTS_FAILED test(s) failed!${NC}"
        exit 1
    fi
}

# Check if gp-setup is available
if ! command -v gp-setup >/dev/null 2>&1; then
    log_error "gp-setup command not found in PATH"
    log_info "Make sure GlobalProtect OpenConnect is properly installed"
    exit 1
fi

# Run the tests
run_tests
