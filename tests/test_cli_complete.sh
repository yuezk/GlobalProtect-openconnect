#!/bin/bash

# Comprehensive CLI Test Script for GlobalProtect OpenConnect
# This script tests the CLI functionality in the pixi environment

set -e  # Exit on any error

echo "=========================================="
echo "GlobalProtect CLI Comprehensive Test"
echo "=========================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Function to run a test
run_test() {
    local test_name="$1"
    local test_command="$2"

    print_status "Running test: $test_name"

    if eval "$test_command"; then
        print_success "✓ $test_name"
        ((TESTS_PASSED++))
    else
        print_error "✗ $test_name"
        ((TESTS_FAILED++))
    fi
    echo
}

print_status "Starting CLI test suite..."
echo

# Test 1: Environment Check
print_status "=== Test 1: Environment Check ==="
run_test "Pixi environment activation" "pixi info"
run_test "Conda prefix check" "test -n \"\$CONDA_PREFIX\""
run_test "PKG_CONFIG_PATH availability" "pixi run check-pkgconfig"

# Test 2: Build Tests
print_status "=== Test 2: Build Tests ==="
run_test "Clean build environment" "pixi run clean"
run_test "CLI build process" "pixi run build-cli"
run_test "Binary files exist" "test -f target/release/gpclient && test -f target/release/gpservice && test -f target/release/gpauth"

# Test 3: Binary Verification
print_status "=== Test 3: Binary Verification ==="
run_test "gpclient executable" "test -x target/release/gpclient"
run_test "gpservice executable" "test -x target/release/gpservice"
run_test "gpauth executable" "test -x target/release/gpauth"

# Test 4: Version Tests
print_status "=== Test 4: Version Tests ==="
run_test "gpclient version check" "target/release/gpclient --version | grep -q '2.4.4'"
run_test "gpservice version check" "target/release/gpservice --version | grep -q '2.4.4'"
run_test "gpauth version check" "target/release/gpauth --version | grep -q '2.4.4'"

# Test 5: Help Documentation
print_status "=== Test 5: Help Documentation ==="
run_test "gpclient help available" "target/release/gpclient --help | grep -q 'GlobalProtect VPN client'"
run_test "gpservice help available" "target/release/gpservice --help | grep -q 'Usage:'"
run_test "gpauth help available" "target/release/gpauth --help | grep -q 'authentication component'"

# Test 6: Command Structure Tests
print_status "=== Test 6: Command Structure Tests ==="
run_test "gpclient subcommands" "target/release/gpclient --help | grep -q 'connect'"
run_test "gpclient subcommands" "target/release/gpclient --help | grep -q 'disconnect'"
run_test "gpauth server argument" "target/release/gpauth --help | grep -q '<SERVER>'"

# Test 7: Dependency Linking Tests
print_status "=== Test 7: Dependency Linking Tests ==="
run_test "OpenConnect library linking" "ldd target/release/gpclient | grep -q 'libopenconnect'"
run_test "SSL library linking" "ldd target/release/gpclient | grep -q 'libssl'"
run_test "Dependencies from pixi environment" "ldd target/release/gpclient | grep -q '.pixi/envs/default'"

# Test 8: Binary Size and Strip Status
print_status "=== Test 8: Binary Analysis ==="
GPCLIENT_SIZE=$(stat -c%s target/release/gpclient)
GPSERVICE_SIZE=$(stat -c%s target/release/gpservice)
GPAUTH_SIZE=$(stat -c%s target/release/gpauth)

run_test "gpclient reasonable size (>1MB, <10MB)" "test $GPCLIENT_SIZE -gt 1000000 && test $GPCLIENT_SIZE -lt 10000000"
run_test "gpservice reasonable size (>1MB, <10MB)" "test $GPSERVICE_SIZE -gt 1000000 && test $GPSERVICE_SIZE -lt 10000000"
run_test "gpauth reasonable size (>1MB, <10MB)" "test $GPAUTH_SIZE -gt 1000000 && test $GPAUTH_SIZE -lt 10000000"

print_status "Binary sizes:"
echo "  gpclient: $(numfmt --to=iec $GPCLIENT_SIZE)"
echo "  gpservice: $(numfmt --to=iec $GPSERVICE_SIZE)"
echo "  gpauth: $(numfmt --to=iec $GPAUTH_SIZE)"
echo

# Test 9: Package Creation Tests
print_status "=== Test 9: Package Creation Tests ==="
run_test "CLI package creation" "pixi run package-cli"
run_test "Package file exists" "test -f output/linux-64/globalprotect-openconnect-cli-*.conda"
run_test "Package reasonable size" "test \$(stat -c%s output/linux-64/globalprotect-openconnect-cli-*.conda) -gt 1000000"

# Test 10: Package Content Verification
print_status "=== Test 10: Package Content Verification ==="
if ls output/linux-64/globalprotect-openconnect-cli-*.conda >/dev/null 2>&1; then
    PACKAGE_FILE=$(ls output/linux-64/globalprotect-openconnect-cli-*.conda | head -1)
    PACKAGE_SIZE=$(stat -c%s "$PACKAGE_FILE")

    print_status "Package file: $(basename "$PACKAGE_FILE")"
    print_status "Package size: $(numfmt --to=iec $PACKAGE_SIZE)"

    run_test "Package inspection" "pixi run inspect-package"
else
    print_error "No package file found for content verification"
    ((TESTS_FAILED++))
fi

# Test 11: Error Handling Tests
print_status "=== Test 11: Error Handling Tests ==="
run_test "gpclient invalid option" "! target/release/gpclient --invalid-option 2>/dev/null"
run_test "gpauth missing server" "! target/release/gpauth 2>/dev/null"
run_test "gpclient help for invalid subcommand" "! target/release/gpclient invalid-command 2>/dev/null"

# Test 12: Workflow Integration Tests
print_status "=== Test 12: Workflow Integration Tests ==="
run_test "Complete CLI workflow" "pixi run cli-workflow"

# Test 13: Environment Isolation Tests
print_status "=== Test 13: Environment Isolation Tests ==="
run_test "Pixi environment variables" "test -n \"\$CONDA_PREFIX\""
run_test "Rust toolchain from pixi" "which rustc | grep -q '.pixi'"
run_test "Cargo from pixi environment" "which cargo | grep -q '.pixi'"

# Final Results Summary
echo
echo "=========================================="
echo "TEST RESULTS SUMMARY"
echo "=========================================="

TOTAL_TESTS=$((TESTS_PASSED + TESTS_FAILED))

print_status "Total tests run: $TOTAL_TESTS"
print_success "Tests passed: $TESTS_PASSED"

if [ $TESTS_FAILED -gt 0 ]; then
    print_error "Tests failed: $TESTS_FAILED"
    echo
    print_error "Some tests failed. Please review the output above."
    exit 1
else
    print_success "All tests passed!"
    echo
    print_success "✓ CLI build is working correctly with pixi environment"
    print_success "✓ All binaries are functional and properly linked"
    print_success "✓ Package creation is successful"
    print_success "✓ Environment isolation is working"
    echo
    print_success "🎉 GlobalProtect CLI is ready for production use!"
fi

# Additional Information
echo
echo "=========================================="
echo "ADDITIONAL INFORMATION"
echo "=========================================="
print_status "Available pixi tasks:"
pixi task list | grep -E "(build|test|package|clean)" || true

echo
print_status "Environment details:"
echo "  Pixi version: $(pixi --version)"
echo "  Rust version: $(rustc --version 2>/dev/null || echo 'Not available')"
echo "  Cargo version: $(cargo --version 2>/dev/null || echo 'Not available')"

if [ -f "$PACKAGE_FILE" ]; then
    echo
    print_status "Package ready for distribution:"
    echo "  File: $PACKAGE_FILE"
    echo "  Size: $(numfmt --to=iec $PACKAGE_SIZE)"
    echo "  Installation: conda install $PACKAGE_FILE"
fi

echo
print_status "Test completed at: $(date)"
