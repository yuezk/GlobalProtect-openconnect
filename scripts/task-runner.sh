#!/bin/bash

# GlobalProtect OpenConnect - Task Runner
# This script provides enhanced task execution with better error handling and logging

set -euo pipefail

# Script metadata
SCRIPT_NAME="task-runner"
VERSION="1.0.0"
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

# Logging configuration
LOG_DIR="$PROJECT_ROOT/logs"
LOG_FILE="$LOG_DIR/task-runner.log"
VERBOSE=false
DRY_RUN=false

# Ensure log directory exists
mkdir -p "$LOG_DIR"

# Helper functions
log_info() {
    local msg="$1"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "${BLUE}[INFO]${NC} $msg"
    echo "[$timestamp] [INFO] $msg" >> "$LOG_FILE"
}

log_success() {
    local msg="$1"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "${GREEN}[SUCCESS]${NC} $msg"
    echo "[$timestamp] [SUCCESS] $msg" >> "$LOG_FILE"
}

log_warning() {
    local msg="$1"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "${YELLOW}[WARNING]${NC} $msg"
    echo "[$timestamp] [WARNING] $msg" >> "$LOG_FILE"
}

log_error() {
    local msg="$1"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "${RED}[ERROR]${NC} $msg" >&2
    echo "[$timestamp] [ERROR] $msg" >> "$LOG_FILE"
}

log_debug() {
    local msg="$1"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    if [[ "$VERBOSE" == "true" ]]; then
        echo -e "${GRAY}[DEBUG]${NC} $msg"
    fi
    echo "[$timestamp] [DEBUG] $msg" >> "$LOG_FILE"
}

# Print header
print_header() {
    echo -e "${CYAN}=============================================${NC}"
    echo -e "${CYAN}  GlobalProtect OpenConnect Task Runner${NC}"
    echo -e "${CYAN}  Version: $VERSION${NC}"
    echo -e "${CYAN}=============================================${NC}"
    echo
}

# Print usage information
show_usage() {
    cat << EOF
Usage: $0 [OPTIONS] <task|workflow> [task-args...]

DESCRIPTION:
    Enhanced task runner for GlobalProtect OpenConnect project with
    better error handling, logging, and workflow management.

OPTIONS:
    -v, --verbose       Enable verbose output
    -n, --dry-run       Show what would be executed without running
    -h, --help          Show this help
    --log-file FILE     Custom log file path (default: logs/task-runner.log)

WORKFLOWS:
    cli-dev             Development workflow for CLI
    cli-ship            Complete CLI shipping workflow
    cli-deploy          CLI deployment workflow
    full-dev            Full development workflow (with GUI)
    full-ship           Full shipping workflow
    docs                Documentation generation workflow
    clean-all           Complete cleanup workflow

SINGLE TASKS:
    build-cli           Build CLI binaries only
    build-full          Build full application
    test-cli            Test CLI functionality
    package-cli         Package CLI for distribution
    package-full        Package full application
    publish-cli         Publish CLI package to prefix.dev
    verify-env          Verify build environment
    setup-env           Setup build environment

EXAMPLES:
    $0 cli-dev                      # Run CLI development workflow
    $0 --verbose cli-ship           # Run CLI shipping with verbose output
    $0 --dry-run full-dev           # Show what full-dev would do
    $0 build-cli                    # Just build CLI binaries
    $0 verify-env                   # Check environment setup

LOG FILES:
    Default log location: $LOG_FILE
    Use --log-file to specify custom location

For more information, see docs/developers-guide.adoc
EOF
}

# Verify pixi environment
verify_pixi_env() {
    log_debug "Verifying pixi environment"

    if ! command -v pixi >/dev/null 2>&1; then
        log_error "pixi command not found. Please install pixi first."
        exit 1
    fi

    if [[ -z "${CONDA_PREFIX:-}" ]]; then
        log_error "CONDA_PREFIX not set. Please run this script from within a pixi environment."
        exit 1
    fi

    log_debug "Pixi environment verified"
}

# Execute pixi task with error handling
run_pixi_task() {
    local task="$1"
    local start_time=$(date +%s)

    log_info "Starting task: $task"

    if [[ "$DRY_RUN" == "true" ]]; then
        log_info "[DRY RUN] Would execute: pixi run $task"
        return 0
    fi

    if pixi run "$task" 2>&1 | tee -a "$LOG_FILE"; then
        local end_time=$(date +%s)
        local duration=$((end_time - start_time))
        log_success "Task '$task' completed in ${duration}s"
        return 0
    else
        local exit_code=$?
        log_error "Task '$task' failed with exit code $exit_code"
        return $exit_code
    fi
}

# Execute workflow (series of tasks)
run_workflow() {
    local workflow="$1"
    local tasks=()

    case "$workflow" in
        "cli-dev")
            tasks=("clean" "build-cli" "test-cli")
            ;;
        "cli-ship")
            tasks=("clean" "build-cli" "test-cli" "package-cli" "show-package-info")
            ;;
        "cli-deploy")
            tasks=("clean" "build-cli" "test-cli" "package-cli" "install-global-cli")
            ;;
        "full-dev")
            tasks=("clean" "setup" "build-frontend" "build-rust" "test")
            ;;
        "full-ship")
            tasks=("clean" "setup" "build" "test" "package" "show-package-info")
            ;;
        "docs")
            tasks=("clean-docs" "docs-html" "docs-pdf")
            ;;
        "clean-all")
            tasks=("clean" "clean-docs")
            ;;
        *)
            log_error "Unknown workflow: $workflow"
            return 1
            ;;
    esac

    log_info "Starting workflow: $workflow (${#tasks[@]} tasks)"
    local failed_tasks=()
    local start_time=$(date +%s)

    for task in "${tasks[@]}"; do
        if ! run_pixi_task "$task"; then
            failed_tasks+=("$task")
            log_error "Workflow '$workflow' failed at task '$task'"
            break
        fi
    done

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))

    if [[ ${#failed_tasks[@]} -eq 0 ]]; then
        log_success "Workflow '$workflow' completed successfully in ${duration}s"
        return 0
    else
        log_error "Workflow '$workflow' failed after ${duration}s"
        return 1
    fi
}

# Environment verification workflow
verify_environment() {
    log_info "Verifying build environment"

    local checks=(
        "verify-pkgconfig"
        "test-pkgconfig"
        "verify-webkit-deps"
        "debug-env"
    )

    local failed=0
    for check in "${checks[@]}"; do
        if ! run_pixi_task "$check"; then
            failed=$((failed + 1))
        fi
    done

    if [[ $failed -eq 0 ]]; then
        log_success "Environment verification passed"
        return 0
    else
        log_error "Environment verification failed ($failed checks failed)"
        return 1
    fi
}

# Main execution function
main() {
    local command=""

    # Parse command line arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -v|--verbose)
                VERBOSE=true
                shift
                ;;
            -n|--dry-run)
                DRY_RUN=true
                shift
                ;;
            --log-file)
                LOG_FILE="$2"
                shift 2
                ;;
            -h|--help)
                show_usage
                exit 0
                ;;
            *)
                command="$1"
                shift
                break
                ;;
        esac
    done

    if [[ -z "$command" ]]; then
        log_error "No command specified"
        show_usage
        exit 1
    fi

    # Initialize logging
    log_info "Task runner started (PID: $$)"
    log_debug "Project root: $PROJECT_ROOT"
    log_debug "Log file: $LOG_FILE"
    log_debug "Verbose: $VERBOSE"
    log_debug "Dry run: $DRY_RUN"

    # Change to project root
    cd "$PROJECT_ROOT"

    # Verify environment
    verify_pixi_env

    # Execute command
    case "$command" in
        # Workflows
        "cli-dev"|"cli-ship"|"cli-deploy"|"full-dev"|"full-ship"|"docs"|"clean-all")
            run_workflow "$command"
            ;;
        # Environment tasks
        "verify-env")
            verify_environment
            ;;
        "setup-env")
            run_pixi_task "setup-env"
            ;;
        # Single tasks
        "build-cli"|"build-full"|"test-cli"|"package-cli"|"package-full"|"publish-cli")
            case "$command" in
                "build-full") command="build" ;;
                "package-full") command="package" ;;
            esac
            run_pixi_task "$command"
            ;;
        *)
            # Try to run as direct pixi task
            log_info "Attempting to run as pixi task: $command"
            run_pixi_task "$command"
            ;;
    esac

    local exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        log_success "Task runner completed successfully"
    else
        log_error "Task runner failed with exit code $exit_code"
    fi

    exit $exit_code
}

# Print header
print_header

# Run main function with all arguments
main "$@"
