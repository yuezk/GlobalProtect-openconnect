#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
#[path = "privileged_unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(not(any(
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd",
  target_os = "macos",
  windows
)))]
#[path = "default.rs"]
mod platform;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", test))]
mod selection;

pub(crate) use platform::builder;
#[cfg(windows)]
pub(crate) use platform::recover_network;
