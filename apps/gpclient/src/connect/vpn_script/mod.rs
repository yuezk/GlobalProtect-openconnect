#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(all(unix, not(target_os = "macos")))]
#[path = "default.rs"]
mod platform;

#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
