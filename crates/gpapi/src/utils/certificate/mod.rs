mod client_key;
mod issuer;
pub(super) use client_key::{ClientKey, validate_certificate};

pub(super) mod pkcs12;
mod pkcs12_format;

#[cfg(any(
  target_os = "macos",
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd"
))]
#[path = "gnutls.rs"]
mod platform_import;
#[cfg(not(any(
  target_os = "macos",
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd"
)))]
#[path = "portable_import.rs"]
mod platform_import;

pub(super) use platform_import::{archive, decrypt as decrypt_key};
