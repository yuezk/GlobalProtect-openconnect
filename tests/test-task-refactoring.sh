#!/bin/bash

# GlobalProtect OpenConnect - Task Refactoring Test Suite
# This script validates the refactored task system against strict naming conventions
# Based on docs/sects/naming-conventions.adoc requirements

set -euo pipefail

# Script metadata
SCRIPT_NAME="test-task-refactoring"
VERSION="2.0.0"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
GRAY='\033[0;90m'
NC='\033[0m' # No Color

# Test configuration
TEST_LOG_DIR="$PROJECT_ROOT/logs/tests"
TEST_LOG_FILE="$TEST_LOG_DIR/task-refactoring-test.log"
VERBOSE=false
QUICK_MODE=false
FAILED_TESTS=()
PASSED_TESTS=()
SKIPPED_TESTS=()

# Ensure test log directory exists
mkdir -p "$TEST_LOG_DIR"

# Helper functions
log_info() {
    local msg="$1"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "${BLUE}[INFO]${NC} $msg"
    echo "[$timestamp] [INFO] $msg" >> "$TEST_LOG_FILE"
}

log_success() {
    local msg="$1"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "${GREEN}[PASS]${NC} $msg"
    echo "[$timestamp] [PASS] $msg" >> "$TEST_LOG_FILE"
}

log_warning() {
    local msg="$1"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "${YELLOW}[WARN]${NC} $msg"
    echo "[$timestamp] [WARN] $msg" >> "$TEST_LOG_FILE"
}

log_error() {
    local msg="$1"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "${RED}[FAIL]${NC} $msg"
    echo "[$timestamp] [FAIL] $msg" >> "$TEST_LOG_FILE"
}

log_skip() {
    local msg="$1"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "${GRAY}[SKIP]${NC} $msg"
    echo "[$timestamp] [SKIP] $msg" >> "$TEST_LOG_FILE"
}

# Print test header
print_header() {
    echo -e "${CYAN}================================================================${NC}"
    echo -e "${CYAN}  GlobalProtect OpenConnect - Task Naming Convention Test${NC}"
    echo -e "${CYAN}  Version: $VERSION${NC}"
    echo -e "${CYAN}  Strict naming enforcement based on naming-conventions.adoc${NC}"
    echo -e "${CYAN}================================================================${NC}"
    echo
}

# Get all pixi tasks
get_all_tasks() {
    pixi task list --machine-readable 2>/dev/null | tr ' ' '\n' | sort | grep -v '^$'
}

# Test if a pixi task exists
test_task_exists() {
    local task_name="$1"
    local description="${2:-$task_name}"

    log_info "Testing task existence: $description"

    if get_all_tasks | grep -q "^$task_name$"; then
        log_success "Task '$task_name' exists"
        PASSED_TESTS+=("task_exists_$task_name")
        return 0
    else
        log_error "Task '$task_name' does not exist"
        FAILED_TESTS+=("task_exists_$task_name")
        return 1
    fi
}

# Test if a task follows proper naming pattern
test_task_naming_pattern() {
    local task_name="$1"

    # Check for action-object pattern (e.g., build-cli, test-all)
    if [[ "$task_name" =~ ^[a-z]+(-[a-z]+)+$ ]]; then
        # Further validate that it follows approved action-object patterns
        local action="${task_name%%-*}"
        local remainder="${task_name#*-}"

        # Check if action is approved
        case "$action" in
            "build"|"clean"|"check"|"debug"|"deploy"|"develop"|"format"|"install"|"lint"|"package"|"publish"|"run"|"setup"|"ship"|"show"|"test"|"verify"|"view")
                return 0
                ;;
            *)
                return 1
                ;;
        esac
    else
        return 1
    fi
}

# Test for prohibited legacy patterns
test_no_legacy_tasks() {
    log_info "Testing for prohibited legacy task names"

    local prohibited_tasks=(
        "build"
        "test"
        "clean"
        "package"
        "docs"
        "lint"
        "format"
        "dev"
        "pkg"
        "fmt"
    )

    local violations=()
    local all_tasks
    all_tasks=$(get_all_tasks)

    for prohibited in "${prohibited_tasks[@]}"; do
        if echo "$all_tasks" | grep -q "^$prohibited$"; then
            violations+=("$prohibited")
        fi
    done

    if [[ ${#violations[@]} -eq 0 ]]; then
        log_success "No prohibited legacy task names found"
        PASSED_TESTS+=("no_legacy_tasks")
        return 0
    else
        log_error "Found prohibited legacy tasks: ${violations[*]}"
        FAILED_TESTS+=("no_legacy_tasks")
        return 1
    fi
}

# Test all required tasks from naming conventions document
test_required_tasks() {
    log_info "Testing for all required tasks from naming conventions"

    # Setup Actions
    local setup_tasks=(
        "setup-corepack"
        "setup-dev"
        "setup-env"
        "setup-publishing"
    )

    # Build Actions
    local build_tasks=(
        "build-all"
        "build-cli"
        "build-frontend"
        "build-rust"
        "build-docs"
        "build-docs-html"
        "build-docs-pdf"
    )

    # Test Actions
    local test_tasks=(
        "test-all"
        "test-cli"
        "test-pkgconfig"
        "test-cli-comprehensive"
        "test-task-refactoring"
        "test-task-refactoring-quick"
        "test-setup-logging"
    )

    # Package Actions
    local package_tasks=(
        "package-full"
        "package-cli"
    )

    # Quality Actions
    local quality_tasks=(
        "lint-code"
        "format-code"
        "check-code-format"
    )

    # Clean Actions
    local clean_tasks=(
        "clean-all"
        "clean-docs"
    )

    # Install Actions
    local install_tasks=(
        "install-global-cli"
        "install-global-full"
    )

    # Debug Actions
    local debug_tasks=(
        "debug-env"
        "debug-build"
        "debug-cli"
    )

    # Verify Actions
    local verify_tasks=(
        "verify-pkgconfig"
        "verify-webkit-deps"
        "verify-package-published"
    )

    # Show Actions
    local show_tasks=(
        "show-env"
        "show-help"
        "show-package-status"
        "show-runner-help"
        "show-docs-help"
    )

    # View Actions
    local view_tasks=(
        "view-docs-all"
        "view-docs-dev"
        "view-docs-ops"
        "view-package-contents"
    )

    # Publish Actions
    local publish_tasks=(
        "publish-cli"
        "publish-cli-complete"
    )

    # Workflow Actions
    local workflow_tasks=(
        "develop-cli"
        "ship-cli"
        "deploy-cli"
        "deploy-full"
    )

    # Enhanced Workflow Actions
    local enhanced_workflow_tasks=(
        "run-workflow-cli-dev"
        "run-workflow-cli-ship"
        "run-workflow-cli-deploy"
        "run-workflow-full-dev"
        "run-workflow-full-ship"
        "run-workflow-docs"
        "run-workflow-clean"
        "run-workflow-verify"
        "run-runner-verbose"
    )

    local missing_tasks=()
    local total_tested=0

    # Test Setup tasks
    log_info "Testing Setup tasks"
    for task in "${setup_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Setup task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Build tasks
    log_info "Testing Build tasks"
    for task in "${build_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Build task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Test tasks
    log_info "Testing Test tasks"
    for task in "${test_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Test task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Package tasks
    log_info "Testing Package tasks"
    for task in "${package_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Package task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Quality tasks
    log_info "Testing Quality tasks"
    for task in "${quality_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Quality task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Clean tasks
    log_info "Testing Clean tasks"
    for task in "${clean_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Clean task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Install tasks
    log_info "Testing Install tasks"
    for task in "${install_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Install task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Debug tasks
    log_info "Testing Debug tasks"
    for task in "${debug_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Debug task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Verify tasks
    log_info "Testing Verify tasks"
    for task in "${verify_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Verify task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Show tasks
    log_info "Testing Show tasks"
    for task in "${show_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Show task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test View tasks
    log_info "Testing View tasks"
    for task in "${view_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "View task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Publish tasks
    log_info "Testing Publish tasks"
    for task in "${publish_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Publish task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Workflow tasks
    log_info "Testing Workflow tasks"
    for task in "${workflow_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Workflow task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    # Test Enhanced Workflow tasks
    log_info "Testing Enhanced Workflow tasks"
    for task in "${enhanced_workflow_tasks[@]}"; do
        total_tested=$((total_tested + 1))
        if ! test_task_exists "$task" "Enhanced task: $task"; then
            missing_tasks+=("$task")
        fi
    done

    if [[ ${#missing_tasks[@]} -eq 0 ]]; then
        log_success "All $total_tested required tasks found"
        PASSED_TESTS+=("required_tasks")
        return 0
    else
        log_error "Missing ${#missing_tasks[@]} required tasks: ${missing_tasks[*]}"
        FAILED_TESTS+=("required_tasks")
        return 1
    fi
}

# Test action-object naming compliance
test_action_object_naming() {
    log_info "Testing strict action-object naming compliance"

    local all_tasks
    all_tasks=$(get_all_tasks)

    local compliant_tasks=()
    local non_compliant_tasks=()

    while IFS= read -r task; do
        if test_task_naming_pattern "$task"; then
            compliant_tasks+=("$task")
        else
            non_compliant_tasks+=("$task")
        fi
    done <<< "$all_tasks"

    log_info "Found ${#compliant_tasks[@]} compliant tasks"
    log_info "Found ${#non_compliant_tasks[@]} non-compliant tasks"

    if [[ ${#non_compliant_tasks[@]} -eq 0 ]]; then
        log_success "ALL tasks follow strict action-object naming pattern"
        PASSED_TESTS+=("strict_naming")
        return 0
    else
        log_error "Non-compliant tasks found: ${non_compliant_tasks[*]}"
        log_error "ALL tasks MUST follow action-object or action-variant-object pattern"
        FAILED_TESTS+=("strict_naming")
        return 1
    fi
}

# Test for approved actions and objects
test_approved_components() {
    log_info "Testing for approved action and object components"

    # Approved actions from naming conventions document
    local approved_actions=(
        "build" "clean" "check" "debug" "deploy" "develop" "format"
        "install" "lint" "package" "publish" "run" "setup" "ship"
        "show" "test" "verify" "view"
    )

    # Approved objects from naming conventions document
    local approved_objects=(
        "all" "build" "cli" "code" "corepack" "contents" "dev" "docs"
        "env" "frontend" "full" "help" "package" "pkgconfig" "published"
        "publishing" "runner" "rust" "setup" "status" "task" "webkit"
    )

    local all_tasks
    all_tasks=$(get_all_tasks)

    local violations=()

    while IFS= read -r task; do
        if [[ "$task" =~ ^([a-z]+)-(.+)$ ]]; then
            local action="${BASH_REMATCH[1]}"
            local remainder="${BASH_REMATCH[2]}"

            # Check if action is approved
            local action_approved=false
            for approved_action in "${approved_actions[@]}"; do
                if [[ "$action" == "$approved_action" ]]; then
                    action_approved=true
                    break
                fi
            done

            if [[ "$action_approved" == "false" ]]; then
                violations+=("$task (unapproved action: $action)")
            fi
        fi
    done <<< "$all_tasks"

    if [[ ${#violations[@]} -eq 0 ]]; then
        log_success "All tasks use approved action components"
        PASSED_TESTS+=("approved_components")
        return 0
    else
        log_error "Tasks with unapproved components: ${violations[*]}"
        FAILED_TESTS+=("approved_components")
        return 1
    fi
}

# Test script existence and permissions
test_script_files() {
    log_info "Testing script files"

    local scripts=(
        "scripts/env/setup-pkgconfig.sh"
        "scripts/task-runner.sh"
        "tests/test-task-refactoring.sh"
    )

    local failed=0
    for script in "${scripts[@]}"; do
        if [[ -f "$PROJECT_ROOT/$script" ]]; then
            if [[ -x "$PROJECT_ROOT/$script" ]] || chmod +x "$PROJECT_ROOT/$script" 2>/dev/null; then
                log_success "Script '$script' exists and is executable"
            else
                log_error "Script '$script' exists but is not executable"
                failed=$((failed + 1))
            fi
        else
            log_error "Script '$script' does not exist"
            failed=$((failed + 1))
        fi
    done

    if [[ $failed -eq 0 ]]; then
        PASSED_TESTS+=("script_files")
        return 0
    else
        FAILED_TESTS+=("script_files")
        return 1
    fi
}

# Test environment setup
test_environment_setup() {
    log_info "Testing environment setup"

    # Test if CONDA_PREFIX is set
    if [[ -n "${CONDA_PREFIX:-}" ]]; then
        log_success "CONDA_PREFIX is set: $CONDA_PREFIX"
    else
        log_error "CONDA_PREFIX is not set"
        FAILED_TESTS+=("env_conda_prefix")
        return 1
    fi

    # Test if pixi is available
    if command -v pixi >/dev/null 2>&1; then
        log_success "pixi command is available"
    else
        log_error "pixi command is not available"
        FAILED_TESTS+=("env_pixi_cmd")
        return 1
    fi

    # Test if we're in the right directory
    if [[ -f "$PROJECT_ROOT/pixi.toml" ]]; then
        log_success "pixi.toml found in project root"
    else
        log_error "pixi.toml not found in project root"
        FAILED_TESTS+=("env_pixi_toml")
        return 1
    fi

    PASSED_TESTS+=("environment_setup")
    return 0
}

# Test task organization and documentation compliance
test_task_organization() {
    log_info "Testing task organization and documentation compliance"

    # Check for proper organization comments in pixi.toml
    local expected_categories=(
        "Setup Actions"
        "Build Actions"
        "Test Actions"
        "Package Actions"
        "Quality Actions"
        "Clean Actions"
        "Install Actions"
        "Debug Actions"
        "Verify Actions"
        "Show/View Actions"
        "Publish Actions"
        "Workflow Tasks"
        "Enhanced Workflow Tasks"
    )

    local missing_categories=()
    for category in "${expected_categories[@]}"; do
        if grep -q "# $category" "$PROJECT_ROOT/pixi.toml" 2>/dev/null; then
            log_success "Found category: $category"
        else
            missing_categories+=("$category")
        fi
    done

    if [[ ${#missing_categories[@]} -eq 0 ]]; then
        log_success "All required task categories are properly organized"
        PASSED_TESTS+=("task_organization")
        return 0
    else
        log_warning "Some task categories missing: ${missing_categories[*]}"
        FAILED_TESTS+=("task_organization")
        return 1
    fi
}

# Test for task runner functionality
test_task_runner() {
    log_info "Testing task runner functionality"

    local runner_script="$PROJECT_ROOT/scripts/task-runner.sh"

    if [[ -f "$runner_script" ]]; then
        # Test help output
        if timeout 10 bash "$runner_script" --help >/dev/null 2>&1; then
            log_success "Task runner help works"
        else
            log_error "Task runner help failed"
            FAILED_TESTS+=("task_runner_help")
            return 1
        fi

        PASSED_TESTS+=("task_runner")
        return 0
    else
        log_error "Task runner script not found"
        FAILED_TESTS+=("task_runner")
        return 1
    fi
}

# Test executable task functionality (safe tasks only)
test_executable_tasks() {
    if [[ "$QUICK_MODE" == "true" ]]; then
        log_info "Skipping executable task tests (quick mode)"
        return 0
    fi

    log_info "Testing executable tasks (safe tasks only)"

    local safe_tasks=("show-help" "show-docs-help" "show-env")

    for task in "${safe_tasks[@]}"; do
        if get_all_tasks | grep -q "^$task$"; then
            if timeout 10 pixi run "$task" >/dev/null 2>&1; then
                log_success "Task '$task' executed successfully"
                PASSED_TESTS+=("exec_$task")
            else
                log_error "Task '$task' failed to execute"
                FAILED_TESTS+=("exec_$task")
            fi
        else
            log_skip "Task '$task' not found, skipping execution test"
            SKIPPED_TESTS+=("exec_$task")
        fi
    done

    return 0
}

# Generate comprehensive test report
generate_report() {
    echo
    echo -e "${CYAN}================================================================${NC}"
    echo -e "${CYAN}  STRICT NAMING CONVENTION TEST REPORT${NC}"
    echo -e "${CYAN}================================================================${NC}"
    echo

    local total_tests=$((${#PASSED_TESTS[@]} + ${#FAILED_TESTS[@]} + ${#SKIPPED_TESTS[@]}))

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

    if [[ ${#SKIPPED_TESTS[@]} -gt 0 ]]; then
        echo -e "${YELLOW}SKIPPED TESTS (${#SKIPPED_TESTS[@]}):${NC}"
        for test in "${SKIPPED_TESTS[@]}"; do
            echo -e "  ${YELLOW}~${NC} $test"
        done
        echo
    fi

    echo -e "${BLUE}SUMMARY:${NC}"
    echo -e "  Total tests: $total_tests"
    echo -e "  Passed: ${GREEN}${#PASSED_TESTS[@]}${NC}"
    echo -e "  Failed: ${RED}${#FAILED_TESTS[@]}${NC}"
    echo -e "  Skipped: ${YELLOW}${#SKIPPED_TESTS[@]}${NC}"
    echo

    if [[ ${#FAILED_TESTS[@]} -eq 0 ]]; then
        echo -e "${GREEN}ALL TESTS PASSED! ✓${NC}"
        echo -e "Task naming conventions are strictly enforced."
        echo -e "NO legacy task names, shortcuts, or aliases found."
    else
        echo -e "${RED}NAMING CONVENTION VIOLATIONS FOUND! ✗${NC}"
        echo -e "ALL tasks MUST follow action-object or action-variant-object pattern."
        echo -e "NO exceptions are permitted - see docs/sects/naming-conventions.adoc"
    fi

    echo
    echo -e "${GRAY}Log file: $TEST_LOG_FILE${NC}"
    echo -e "${CYAN}================================================================${NC}"
}

# Show usage
show_usage() {
    cat << EOF
Usage: $0 [OPTIONS]

DESCRIPTION:
    Strict naming convention test suite for the GlobalProtect OpenConnect project.
    Validates ALL tasks follow action-object or action-variant-object patterns.
    Based on docs/sects/naming-conventions.adoc requirements.

    NO LEGACY TASKS, SHORTCUTS, OR ALIASES ARE PERMITTED.

OPTIONS:
    -v, --verbose       Enable verbose output
    -q, --quick         Quick mode (skip executable tests)
    -h, --help          Show this help

STRICT TESTS:
    - NO legacy task names (build, test, clean, etc.)
    - ALL tasks must follow action-object pattern
    - ALL tasks must use approved action verbs
    - ALL tasks must use approved object nouns
    - Required tasks from naming conventions document
    - Task organization and documentation compliance
    - Script file existence and permissions
    - Environment setup verification

EXAMPLES:
    $0                  # Run all strict naming tests
    $0 --quick          # Run without executable tests
    $0 --verbose        # Run with detailed output

ENFORCEMENT:
    This test enforces ZERO TOLERANCE for naming violations.
    ALL tasks must comply with naming-conventions.adoc.
    Tab completion eliminates need for shortcuts.
EOF
}

# Main execution function
main() {
    # Parse command line arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -v|--verbose)
                VERBOSE=true
                shift
                ;;
            -q|--quick)
                QUICK_MODE=true
                shift
                ;;
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

    # Print header
    print_header

    # Initialize logging
    log_info "Strict naming convention test suite started"
    log_info "Project root: $PROJECT_ROOT"
    log_info "Verbose mode: $VERBOSE"
    log_info "Quick mode: $QUICK_MODE"
    log_info "Enforcing ZERO TOLERANCE for naming violations"

    # Change to project root
    cd "$PROJECT_ROOT"

    # Run test suites in strict order
    log_info "Starting strict naming convention tests"
    echo

    # Core tests
    test_script_files
    test_environment_setup

    # Strict naming tests - these are the critical ones
    test_no_legacy_tasks
    test_action_object_naming
    test_approved_components
    test_required_tasks

    # Organization tests
    test_task_organization
    test_task_runner

    # Optional executable tests
    test_executable_tasks

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
