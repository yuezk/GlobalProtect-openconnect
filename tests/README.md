# GlobalProtect OpenConnect Tests

This directory contains test scripts and test infrastructure for the GlobalProtect OpenConnect project.

## Test Scripts

### CLI Testing Scripts

#### `test_cli_final.sh`
**Purpose**: Comprehensive CLI functionality test suite  
**Usage**: `./tests/test_cli_final.sh` or `pixi run test-cli-comprehensive`  
**Description**: Runs a complete test suite for all CLI components including build verification, functionality testing, and package creation.

**Test Coverage**:
- Pixi environment validation
- CLI build process verification
- Binary functionality testing
- Version information validation
- Help documentation verification
- Command structure testing
- Library dependency verification
- Binary size validation
- Conda package creation testing

**Exit Codes**:
- `0`: All tests passed
- `1`: One or more tests failed

#### `test_cli_complete.sh`
**Purpose**: Extended CLI test suite with detailed reporting  
**Usage**: `./tests/test_cli_complete.sh`  
**Description**: A more comprehensive test suite with colored output, detailed reporting, and extensive validation checks.

**Test Coverage**:
- Environment and dependency checks
- Build system validation
- Binary verification and analysis
- Functional testing with real command execution
- Performance and resource usage validation
- Package creation and inspection
- Error handling validation
- Integration testing

**Features**:
- Colored console output for better readability
- Detailed test result reporting
- Performance metrics collection
- Comprehensive error diagnostics

## Running Tests

### Using Pixi (Recommended)

```bash
# Run comprehensive CLI test suite
pixi run test-cli-comprehensive

# Run basic CLI tests
pixi run test-cli

# Run individual test components
pixi run build-cli && pixi run test-cli
```

### Direct Execution

```bash
# Make scripts executable
chmod +x tests/*.sh

# Run comprehensive test
./tests/test_cli_final.sh

# Run extended test suite
./tests/test_cli_complete.sh
```

### CI/CD Integration

These test scripts are designed for integration with CI/CD pipelines:

```yaml
# Example GitHub Actions step
- name: Run CLI Tests
  run: pixi run test-cli-comprehensive

# Example GitLab CI step
test_cli:
  script:
    - pixi install
    - pixi run test-cli-comprehensive
```

## Test Requirements

### System Requirements
- Linux environment (primary testing platform)
- Pixi package manager installed
- Sufficient disk space for build artifacts (~500MB)
- Network connectivity for dependency installation

### Dependencies
All test dependencies are managed through pixi:
- Rust toolchain (1.80+)
- Build tools (make, pkg-config)
- System libraries (OpenConnect, SSL)
- Testing utilities (built into scripts)

## Test Output and Reporting

### Standard Output Format
```
==========================================
GlobalProtect CLI Final Test Suite
==========================================
[1/10] Testing pixi environment...
✓ PASS: Pixi environment active

[2/10] Cleaning and building CLI...
✓ PASS: CLI build successful
...
```

### Test Metrics
- **Total Tests**: Number of test cases executed
- **Pass Rate**: Percentage of successful tests
- **Build Time**: Time to complete CLI build
- **Binary Sizes**: Size of generated executables
- **Package Size**: Size of generated conda package

### Artifacts Generated
- `target/release/gpclient` - CLI client binary
- `target/release/gpservice` - Service component binary
- `target/release/gpauth` - Authentication binary
- `output/linux-64/*.conda` - Conda package

## Adding New Tests

### Test Script Structure
```bash
#!/bin/bash
# Test description and purpose

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Test function
test_result() {
    if [ $? -eq 0 ]; then
        echo "✓ PASS: $1"
        ((TESTS_PASSED++))
    else
        echo "✗ FAIL: $1"
        ((TESTS_FAILED++))
    fi
}

# Test implementation
echo "Testing feature..."
command_to_test
test_result "Feature description"
```

### Guidelines for New Tests
1. **Clear Purpose**: Each test should have a specific, documented purpose
2. **Idempotent**: Tests should be runnable multiple times with same results
3. **Self-contained**: Tests should not depend on external state
4. **Fast Execution**: Aim for quick feedback cycles
5. **Clear Output**: Provide clear pass/fail indication and error messages

### Integration with Pixi
Add new test tasks to `pixi.toml`:
```toml
[tasks.test-new-feature]
cmd = "chmod +x tests/test_new_feature.sh && ./tests/test_new_feature.sh"
cwd = "."
```

## Troubleshooting Tests

### Common Issues

#### Test Environment Problems
```bash
# Clean and reinitialize environment
pixi clean
pixi install

# Verify environment
pixi info
```

#### Build Failures
```bash
# Check dependency availability
pixi run verify-pkgconfig

# Clean build artifacts
pixi run clean
```

#### Permission Issues
```bash
# Ensure scripts are executable
chmod +x tests/*.sh

# Check file permissions
ls -la tests/
```

### Debug Mode
Enable verbose output for debugging:
```bash
# Enable debug logging
export RUST_LOG=debug
export GP_LOG_LEVEL=debug

# Run tests with verbose output
./tests/test_cli_final.sh
```

## Performance Benchmarks

### Expected Performance Metrics
- **CLI Build Time**: ~54 seconds (full rebuild)
- **Test Execution Time**: ~2-3 minutes (comprehensive suite)
- **Binary Sizes**: 3.7-4.0 MB per binary
- **Package Size**: ~3.7 MB (compressed conda package)

### Performance Monitoring
Tests include performance validation to ensure:
- Build times remain reasonable
- Binary sizes are optimized
- Memory usage is within limits
- Test execution completes promptly

## Test History and Versions

### Version 2.4.4
- Complete CLI test suite implementation
- Pixi integration testing
- Conda package validation
- Performance benchmarking

### Future Enhancements
- GUI testing framework (when GUI components are implemented)
- Integration testing with real VPN servers
- Cross-platform testing automation
- Performance regression testing

## Contributing to Tests

### Test Contributions Welcome
- New test cases for edge conditions
- Performance optimization tests
- Cross-platform compatibility tests
- Integration test scenarios

### Submission Process
1. Create test script following guidelines
2. Add pixi task configuration
3. Update this README with test documentation
4. Submit pull request with test results

## Support

For test-related issues:
- **GitHub Issues**: Bug reports for test failures
- **GitHub Discussions**: Questions about test implementation
- **Documentation**: See developer's guide for detailed testing procedures

---

**Last Updated**: July 12, 2025  
**Test Framework Version**: 2.4.4  
**Supported Platforms**: Linux (primary), macOS, Windows (planned)