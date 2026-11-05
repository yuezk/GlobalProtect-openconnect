pub(super) mod pkcs12;

#[cfg(any(
  target_os = "macos",
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd"
))]
#[path = "gnutls.rs"]
mod legacy_key;
#[cfg(not(any(
  target_os = "macos",
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd"
)))]
#[path = "unsupported.rs"]
mod legacy_key;

pub(super) use legacy_key::decrypt as decrypt_key;
