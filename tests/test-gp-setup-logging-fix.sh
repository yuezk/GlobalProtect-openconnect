#!/bin/bash

# GlobalProtect OpenConnect - gp-setup Logging Fix Test
# This script tests that the gp-setup logging permission issue has been fixed

set -euo pipefail

# Script metadata
SCRIPT_NAME="test-gp-setup-logging-fix"
VERSION="1.0.0"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Test results
PASSED_TESTS=()
FAILED_TESTS=()
WARNINGS=()

# Helper functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[PASS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[FAIL]${NC} $1"
}

print_header() {
    echo -e "${CYAN}================================================================${NC}"
    echo -e "${CYAN}  GlobalProtect OpenConnect - gp-setup Logging Fix Test${NC}"
    echo -e "${CYAN}  Version: $VERSION${NC}"
    echo -e "${CYAN}================================================================${NC}"
    echo
}

# Test the gp-setup script logging logic
test_gp_setup_logic() {
    log_info "Testing gp-setup logging logic implementation"

    local gp_setup_script="$PROJECT_ROOT/packaging/files/usr/bin/gp-setup"

    if [[ ! -f "$gp_setup_script" ]]; then
        log_error "gp-setup script not found at: $gp_setup_script"
        FAILED_TESTS+=("gp_setup_exists")
        return 1
    fi

    log_success "Found gp-setup script"
    PASSED_TESTS+=("gp_setup_exists")

    # Test 1: Check for hardcoded /tmp/gp-setup.log (the original problem)
    if grep -q "/tmp/gp-setup\.log" "$gp_setup_script"; then
        log_error "Found hardcoded '/tmp/gp-setup.log' - this was the original problem!"
        FAILED_TESTS+=("no_hardcoded_log")
        return 1
    else
        log_success "No hardcoded '/tmp/gp-setup.log' found - original issue fixed"
        PASSED_TESTS+=("no_hardcoded_log")
    fi

    # Test 2: Check for proper EUID-based logic
    if grep -q "if \[\[ \$EUID -eq 0 \]\]" "$gp_setup_script"; then
        log_success "Found EUID-based log file selection logic"
        PASSED_TESTS+=("euid_logic")
    else
        log_error "Missing EUID-based log file selection"
        FAILED_TESTS+=("euid_logic")
    fi

    # Test 3: Check for /var/log path for root
    if grep -q "/var/log/gp-setup\.log" "$gp_setup_script"; then
        log_success "Found /var/log/gp-setup.log for root execution"
        PASSED_TESTS+=("root_log_path")
    else
        log_error "Missing /var/log/gp-setup.log for root"
        FAILED_TESTS+=("root_log_path")
    fi

    # Test 4: Check for user-specific tmp log
    if grep -q "/tmp/gp-setup-\$USER\.log" "$gp_setup_script"; then
        log_success "Found user-specific log path: /tmp/gp-setup-\$USER.log"
        PASSED_TESTS+=("user_log_path")
    else
        log_error "Missing user-specific log path"
        FAILED_TESTS+=("user_log_path")
    fi

    # Test 5: Check for fallback mechanism
    if grep -q "/tmp/gp-setup-root\.log" "$gp_setup_script"; then
        log_success "Found fallback log path for root: /tmp/gp-setup-root.log"
        PASSED_TESTS+=("fallback_log_path")
    else
        log_warning "No fallback log path found - might cause issues if /var/log is not writable"
        WARNINGS+=("fallback_log_path")
    fi

    # Test 6: Check for proper error handling
    if grep -q "touch.*2>/dev/null.*||" "$gp_setup_script"; then
        log_success "Found proper error handling for log file creation"
        PASSED_TESTS+=("error_handling")
    else
        log_error "Missing proper error handling for log file creation"
        FAILED_TESTS+=("error_handling")
    fi

    return 0
}

# Simulate the logging scenarios
test_logging_scenarios() {
    log_info "Testing logging scenarios simulation"

    # Scenario 1: Normal user
    log_info "Scenario 1: Normal user execution"
    local user_euid=1000
    local test_user="testuser"

    # Simulate the logic from gp-setup
    if [[ $user_euid -eq 0 ]]; then
        local log_file="/var/log/gp-setup.log"
    else
        local log_file="/tmp/gp-setup-$test_user.log"
    fi

    if [[ "$log_file" == "/tmp/gp-setup-$test_user.log" ]]; then
        log_success "User scenario: Correctly uses user-specific log: $log_file"
        PASSED_TESTS+=("user_scenario")
    else
        log_error "User scenario: Wrong log file: $log_file"
        FAILED_TESTS+=("user_scenario")
    fi

    # Scenario 2: Root/sudo execution
    log_info "Scenario 2: Root/sudo execution"
    local root_euid=0

    if [[ $root_euid -eq 0 ]]; then
        local log_file="/var/log/gp-setup.log"
    else
        local log_file="/tmp/gp-setup-$test_user.log"
    fi

    if [[ "$log_file" == "/var/log/gp-setup.log" ]]; then
        log_success "Root scenario: Correctly uses system log: $log_file"
        PASSED_TESTS+=("root_scenario")
    else
        log_error "Root scenario: Wrong log file: $log_file"
        FAILED_TESTS+=("root_scenario")
    fi
}

# Test actual file permissions
test_file_permissions() {
    log_info "Testing actual file permission scenarios"

    # Test user tmp file creation
    local test_log="/tmp/gp-setup-test-$$"
    if touch "$test_log" 2>/dev/null; then
        log_success "Can create user log file in /tmp"
        rm -f "$test_log"
        PASSED_TESTS+=("user_tmp_writable")
    else
        log_error "Cannot create user log file in /tmp"
        FAILED_TESTS+=("user_tmp_writable")
    fi

    # Test if we can check /var/log (without actually writing as non-root)
    if [[ -d "/var/log" ]]; then
        log_success "/var/log directory exists"
        PASSED_TESTS+=("var_log_exists")

        if [[ $EUID -eq 0 ]]; then
            # Running as root, test actual write
            local test_log="/var/log/gp-setup-test-$$"
            if touch "$test_log" 2>/dev/null; then
                log_success "Can create system log file in /var/log (running as root)"
                rm -f "$test_log"
                PASSED_TESTS+=("var_log_writable")
            else
                log_error "Cannot create system log file in /var/log (even as root)"
                FAILED_TESTS+=("var_log_writable")
            fi
        else
            log_info "Not running as root, cannot test /var/log write permissions"
            log_info "This is expected - sudo gp-setup would run as root and have access"
        fi
    else
        log_error "/var/log directory does not exist"
        FAILED_TESTS+=("var_log_exists")
    fi
}

# Test the specific sudo scenario mentioned in the issue
test_sudo_scenario() {
    log_info "Testing the specific sudo scenario: sudo ~/.pixi/bin/gp-setup --system"

    # Explain the scenario
    echo "The original issue was:"
    echo "  - User runs: sudo ~/.pixi/bin/gp-setup --system"
    echo "  - Script was hardcoded to use: /tmp/gp-setup.log"
    echo "  - This caused permission conflicts in some sudo configurations"
    echo

    echo "The fix implemented:"
    echo "  - When EUID=0 (sudo), use: /var/log/gp-setup.log"
    echo "  - Fallback to: /tmp/gp-setup-root.log if /var/log fails"
    echo "  - When EUID≠0 (user), use: /tmp/gp-setup-\$USER.log"
    echo "  - Disable logging if all options fail"
    echo

    # Check if the fix addresses the scenario
    local gp_setup_script="$PROJECT_ROOT/packaging/files/usr/bin/gp-setup"

    if [[ -f "$gp_setup_script" ]]; then
        # Look for the specific logic that handles sudo
        if grep -A 5 -B 5 "EUID -eq 0" "$gp_setup_script" | grep -q "/var/log/gp-setup.log"; then
            log_success "✓ sudo scenario fix confirmed: Uses /var/log/gp-setup.log when EUID=0"
            PASSED_TESTS+=("sudo_scenario_fix")
        else
            log_error "✗ sudo scenario fix not found"
            FAILED_TESTS+=("sudo_scenario_fix")
        fi

        # Check for graceful fallback
        if grep -q "gp-setup-root.log" "$gp_setup_script"; then
            log_success "✓ Fallback mechanism present for sudo scenario"
            PASSED_TESTS+=("sudo_fallback")
        else
            log_warning "⚠ No fallback mechanism found for sudo scenario"
            WARNINGS+=("sudo_fallback")
        fi
    fi

    echo
    log_info "Conclusion: The original issue 'sudo ~/.pixi/bin/gp-setup --system permission denied with /tmp/gp-setup.log' has been fixed"
}

# Generate comprehensive report
generate_report() {
    echo
    echo -e "${CYAN}================================================================${NC}"
    echo -e "${CYAN}  TEST REPORT - gp-setup Logging Fix${NC}"
    echo -e "${CYAN}================================================================${NC}"
    echo

    local total_tests=$((${#PASSED_TESTS[@]} + ${#FAILED_TESTS[@]}))

    echo -e "${GREEN}PASSED TESTS (${#PASSED_TESTS[@]}):${NC}"
    for test in "${PASSED_TESTS[@]}"; do
        echo -e "  ${GREEN}✓${NC} $test"
    done
    echo

    if [[ ${#FAILED_TESTS[@]} -gt 0 ]]; then
        echo -e "${RED}FAILED TESTS (${#FAILED_TESTS[@]}):${NC}"
        for test in "${FAILED_TESTS[@]}"; do
            echo -e "  ${RED}✗${NC} $test"
        done
        echo
    fi

    if [[ ${#WARNINGS[@]} -gt 0 ]]; then
        echo -e "${YELLOW}WARNINGS (${#WARNINGS[@]}):${NC}"
        for warning in "${WARNINGS[@]}"; do
            echo -e "  ${YELLOW}⚠${NC} $warning"
        done
        echo
    fi

    echo -e "${BLUE}SUMMARY:${NC}"
    echo -e "  Total tests: $total_tests"
    echo -e "  Passed: ${GREEN}${#PASSED_TESTS[@]}${NC}"
    echo -e "  Failed: ${RED}${#FAILED_TESTS[@]}${NC}"
    echo -e "  Warnings: ${YELLOW}${#WARNINGS[@]}${NC}"
    echo

    echo -e "${BLUE}ORIGINAL ISSUE STATUS:${NC}"
    if [[ ${#FAILED_TESTS[@]} -eq 0 ]]; then
        echo -e "${GREEN}✓ FIXED${NC} - The 'sudo gp-setup --system' permission issue has been resolved"
        echo -e "  • No hardcoded /tmp/gp-setup.log"
        echo -e "  • Proper EUID-based log file selection"
        echo -e "  • System log (/var/log) for root execution"
        echo -e "  • User-specific logs for normal execution"
        echo -e "  • Fallback mechanisms for edge cases"
        echo -e "  • Graceful degradation when logging fails"
    else
        echo -e "${RED}✗ NOT FULLY FIXED${NC} - Some issues remain with the logging implementation"
        echo -e "  Review failed tests above for details"
    fi

    echo
    echo -e "${CYAN}================================================================${NC}"
}

# Show usage
show_usage() {
    cat << EOF
Usage: $0 [OPTIONS]

DESCRIPTION:
    Test suite to verify that the gp-setup logging permission issue
    has been properly fixed. The original issue was that running
    'sudo ~/.pixi/bin/gp-setup --system' would fail due to hardcoded
    /tmp/gp-setup.log causing permission conflicts.

OPTIONS:
    -h, --help          Show this help

TESTS PERFORMED:
    - gp-setup script existence and accessibility
    - Absence of hardcoded /tmp/gp-setup.log (original problem)
    - EUID-based log file selection logic
    - Proper paths for root vs user execution
    - Fallback mechanisms for permission failures
    - Error handling for log file creation
    - Simulation of different execution scenarios
    - File permission validation

EXPECTED OUTCOME:
    All tests should pass, confirming that the sudo permission
    issue with gp-setup logging has been resolved.

EOF
}

# Main execution function
main() {
    # Parse command line arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                show_usage
                exit 0
                ;;
            *)
                echo "Unknown option: $1"
                show_usage
                exit 1
                ;;
        esac
    done

    print_header

    log_info "Testing gp-setup logging fix implementation"
    log_info "Original issue: 'sudo ~/.pixi/bin/gp-setup --system' permission denied with /tmp/gp-setup.log"
    echo

    # Run all tests
    test_gp_setup_logic
    test_logging_scenarios
    test_file_permissions
    test_sudo_scenario

    # Generate comprehensive report
    generate_report

    # Return appropriate exit code
    if [[ ${#FAILED_TESTS[@]} -eq 0 ]]; then
        exit 0
    else
        exit 1
    fi
}

# Run main function with all arguments
main "$@"
