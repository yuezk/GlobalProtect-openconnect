#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
#[path = "privileged_unix.rs"]
mod platform;
#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
#[path = "default.rs"]
mod platform;

pub(crate) use platform::trusted_csd_uid;
