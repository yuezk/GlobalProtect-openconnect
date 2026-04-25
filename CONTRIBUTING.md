# Contributing to GlobalProtect-openconnect

Thank you for your interest in contributing! This document provides guidelines and best practices for contributing to this project.

## Development Setup

### Prerequisites

- Rust 1.85 or later
- System dependencies (see README.md)
- Git with submodules support

### Getting Started

```bash
git clone --recursive https://github.com/yuezk/GlobalProtect-openconnect.git
cd GlobalProtect-openconnect
cargo build
cargo test
```

## Code Quality Standards

### Rust Idioms

1. **Error Handling**
   - Use `Result` types for fallible operations
   - Prefer `?` operator over `unwrap()` in production code
   - Use `thiserror` for library errors, `anyhow` for application errors
   - Add context to errors: `.context("Failed to ...")?`

2. **Performance**
   - Avoid unnecessary cloning - use references when possible
   - Use `Cow<str>` when you might not need to allocate
   - Cache compiled regexes using `LazyLock`
   - Profile before optimizing

3. **Safety**
   - Minimize `unsafe` code
   - Document safety invariants for all unsafe blocks
   - Prefer safe abstractions over unsafe when possible

4. **Testing**
   - Write unit tests for new functionality
   - Add integration tests for public APIs
   - Use property-based testing for parsers and encoders

### Code Style

This project uses `rustfmt` for code formatting:

```bash
cargo fmt --all
```

Run clippy to catch common mistakes:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

### Commit Messages

Follow conventional commit format:

```
<type>(<scope>): <subject>

<body>

<footer>
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Test additions or modifications
- `chore`: Build process or tooling changes

Example:
```
feat(auth): add support for FIDO2 authentication

Implements FIDO2/WebAuthn support for hardware security keys.
This allows users to authenticate using YubiKey and similar devices.

Closes #123
```

## Pull Request Process

1. **Before Creating a PR**
   - Ensure all tests pass: `cargo test --workspace`
   - Run clippy: `cargo clippy --workspace --all-targets -- -D warnings`
   - Format code: `cargo fmt --all`
   - Update documentation if needed

2. **PR Description**
   - Clearly describe what the PR does
   - Reference any related issues
   - Include screenshots for UI changes
   - List any breaking changes

3. **Code Review**
   - Address reviewer feedback promptly
   - Keep PRs focused and reasonably sized
   - Rebase on main before merging

## Architecture Guidelines

### Module Organization

```
crates/
  ├── gpapi/        # Core API library (MIT license)
  ├── openconnect/  # FFI bindings to openconnect
  ├── common/       # Shared utilities
  └── auth/         # Authentication implementations

apps/
  ├── gpclient/     # CLI application
  ├── gpservice/    # Background service
  ├── gpauth/       # Authentication helper
  └── gpgui-helper/ # GUI helper (Tauri)
```

### Error Handling

- Use custom error types for libraries
- Derive `thiserror::Error` for structured errors
- Use `anyhow::Result` in applications
- Add context with `.context()` or `.with_context()`

Example:
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Invalid credentials")]
    InvalidCredentials,
    
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    
    #[error("Parse error: {0}")]
    Parse(String),
}
```

### Async Guidelines

- Use `tokio` for async runtime
- Don't block in async functions - use `tokio::fs`, `tokio::spawn_blocking`
- Document async requirements in function docs
- Consider providing both sync and async variants

### Public API Design

- Keep public API surface minimal
- Use builder pattern for complex configurations
- Document all public items
- Add examples in doc comments
- Use semantic versioning

## Testing

### Unit Tests

Place unit tests in the same file as the code:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_input() {
        let result = parse("valid input");
        assert!(result.is_ok());
    }
}
```

### Integration Tests

Place integration tests in `tests/` directory:

```rust
// tests/auth_integration.rs
#[tokio::test]
async fn test_complete_auth_flow() {
    // Test end-to-end authentication
}
```

### Benchmarks

Add benchmarks for performance-critical code:

```bash
cargo bench --package gpapi
```

See `crates/gpapi/benches/` for examples.

## Documentation

- Add doc comments to all public items
- Use examples in doc comments
- Keep README.md up to date
- Document breaking changes in changelog

Example:
```rust
/// Authenticates a user with the portal server.
///
/// # Arguments
///
/// * `server` - The portal server URL
/// * `credentials` - User credentials
///
/// # Examples
///
/// ```no_run
/// use gpapi::auth::authenticate;
///
/// let result = authenticate("vpn.example.com", credentials).await?;
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - Network connection fails
/// - Credentials are invalid
/// - Server is unreachable
pub async fn authenticate(server: &str, credentials: &Credentials) -> Result<AuthData> {
    // ...
}
```

## Security

- Never commit secrets or credentials
- Use `html_escape` for HTML content
- Use `regex::escape` for regex patterns from user input
- Validate all user input
- Report security issues privately to maintainers

## Getting Help

- Check existing issues and documentation
- Ask questions in GitHub discussions
- Join our community chat (if available)

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (GPL-3.0 for applications, MIT for gpapi crate).
