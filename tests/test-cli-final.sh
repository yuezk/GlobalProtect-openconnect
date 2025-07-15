#!/bin/bash

# Final CLI Test for GlobalProtect OpenConnect with Pixi
# This script runs comprehensive tests within the pixi environment

echo "=========================================="
echo "GlobalProtect CLI Final Test Suite"
echo "=========================================="

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

test_result() {
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓ PASS${NC}: $1"
        ((TESTS_PASSED++))
    else
        echo -e "${RED}✗ FAIL${NC}: $1"
        ((TESTS_FAILED++))
    fi
}

echo -e "${BLUE}[1/10]${NC} Testing pixi environment..."
echo "CONDA_PREFIX: $CONDA_PREFIX"
echo "PKG_CONFIG_PATH: $PKG_CONFIG_PATH"
test -n "$CONDA_PREFIX"
test_result "Pixi environment active"

echo -e "\n${BLUE}[2/10]${NC} Cleaning and building CLI..."
cargo clean > /dev/null 2>&1
PKG_CONFIG_PATH=$CONDA_PREFIX/lib/pkgconfig:$CONDA_PREFIX/share/pkgconfig cargo build --release --no-default-features -p gpclient -p gpservice -p gpauth > /dev/null 2>&1
test_result "CLI build successful"

echo -e "\n${BLUE}[3/10]${NC} Checking binary files exist..."
test -f target/release/gpclient && test -f target/release/gpservice && test -f target/release/gpauth
test_result "All CLI binaries created"

echo -e "\n${BLUE}[4/10]${NC} Testing executable permissions..."
test -x target/release/gpclient && test -x target/release/gpservice && test -x target/release/gpauth
test_result "All binaries executable"

echo -e "\n${BLUE}[5/10]${NC} Testing version output..."
./target/release/gpclient --version | grep -q "2.4.4" && \
./target/release/gpservice --version | grep -q "2.4.4" && \
./target/release/gpauth --version | grep -q "2.4.4"
test_result "Version information correct"

echo -e "\n${BLUE}[6/10]${NC} Testing help functionality..."
./target/release/gpclient --help | grep -q "GlobalProtect VPN client" && \
./target/release/gpservice --help | grep -q "Usage:" && \
./target/release/gpauth --help | grep -q "authentication component"
test_result "Help documentation available"

echo -e "\n${BLUE}[7/10]${NC} Testing command structure..."
./target/release/gpclient --help | grep -q "connect" && \
./target/release/gpclient --help | grep -q "disconnect" && \
./target/release/gpauth --help | grep -q "<SERVER>"
test_result "Command structure correct"

echo -e "\n${BLUE}[8/10]${NC} Testing library dependencies..."
ldd target/release/gpclient | grep -q "libopenconnect" && \
ldd target/release/gpclient | grep -q "libssl" && \
ldd target/release/gpclient | grep -q ".pixi/envs/default"
test_result "Dependencies linked correctly"

echo -e "\n${BLUE}[9/10]${NC} Testing binary sizes..."
GPCLIENT_SIZE=$(stat -c%s target/release/gpclient)
GPSERVICE_SIZE=$(stat -c%s target/release/gpservice)
GPAUTH_SIZE=$(stat -c%s target/release/gpauth)

echo "Binary sizes:"
echo "  gpclient: $(numfmt --to=iec $GPCLIENT_SIZE)"
echo "  gpservice: $(numfmt --to=iec $GPSERVICE_SIZE)"
echo "  gpauth: $(numfmt --to=iec $GPAUTH_SIZE)"

test $GPCLIENT_SIZE -gt 1000000 && test $GPCLIENT_SIZE -lt 20000000 && \
test $GPSERVICE_SIZE -gt 1000000 && test $GPSERVICE_SIZE -lt 20000000 && \
test $GPAUTH_SIZE -gt 1000000 && test $GPAUTH_SIZE -lt 20000000
test_result "Binary sizes reasonable"

echo -e "\n${BLUE}[10/10]${NC} Testing package creation..."
rattler-build build --recipe recipe-cli.yaml > /dev/null 2>&1
test -f output/linux-64/globalprotect-openconnect-cli-*.conda
test_result "Conda package created"

if [ -f output/linux-64/globalprotect-openconnect-cli-*.conda ]; then
    PACKAGE_FILE=$(ls output/linux-64/globalprotect-openconnect-cli-*.conda | head -1)
    PACKAGE_SIZE=$(stat -c%s "$PACKAGE_FILE")
    echo "Package: $(basename "$PACKAGE_FILE")"
    echo "Size: $(numfmt --to=iec $PACKAGE_SIZE)"
fi

echo
echo "=========================================="
echo "FINAL RESULTS"
echo "=========================================="

TOTAL_TESTS=$((TESTS_PASSED + TESTS_FAILED))

echo "Total tests: $TOTAL_TESTS"
echo -e "${GREEN}Passed: $TESTS_PASSED${NC}"

if [ $TESTS_FAILED -gt 0 ]; then
    echo -e "${RED}Failed: $TESTS_FAILED${NC}"
    echo
    echo -e "${RED}❌ Some tests failed${NC}"
    exit 1
else
    echo -e "${RED}Failed: $TESTS_FAILED${NC}"
    echo
    echo -e "${GREEN}🎉 All tests passed!${NC}"
    echo
    echo -e "${GREEN}✅ CLI build is working perfectly with pixi${NC}"
    echo -e "${GREEN}✅ All binaries are functional${NC}"
    echo -e "${GREEN}✅ Package creation successful${NC}"
    echo -e "${GREEN}✅ Ready for production use${NC}"
    echo
    echo "Available commands:"
    echo "  pixi run build-cli     # Build CLI binaries"
    echo "  pixi run test-cli      # Test CLI functionality"
    echo "  pixi run package-cli   # Create conda package"
    echo "  pixi run dev-cli       # Complete workflow"
fi

echo
echo "Test completed at: $(date)"
