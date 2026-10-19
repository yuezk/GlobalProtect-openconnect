#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(not(target_os = "macos"))]
#[path = "default.rs"]
mod platform;

pub(super) use platform::builder;
