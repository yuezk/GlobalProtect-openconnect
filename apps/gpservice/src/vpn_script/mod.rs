#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
#[path = "privileged_unix.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(not(any(
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd",
  target_os = "macos"
)))]
#[path = "default.rs"]
mod platform;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", test))]
mod selection;

pub(crate) use platform::builder;
