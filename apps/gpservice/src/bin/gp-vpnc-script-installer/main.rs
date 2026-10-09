#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
#[path = "privileged_unix.rs"]
mod platform;
#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
#[path = "unsupported.rs"]
mod platform;

fn main() {
  platform::run();
}
