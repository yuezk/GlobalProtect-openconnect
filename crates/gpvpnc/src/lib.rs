#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;
#[cfg(all(test, not(windows)))]
#[path = "windows/configuration.rs"]
mod windows_configuration;
