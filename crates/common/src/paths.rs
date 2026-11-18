//! Machine-wide service state shared by the daemon and desktop clients.
use std::path::PathBuf;

pub fn service_lock_file() -> PathBuf {
  #[cfg(unix)]
  {
    PathBuf::from("/var/run/gpservice.lock")
  }
  #[cfg(windows)]
  {
    std::env::var_os("ProgramData")
      .map(PathBuf::from)
      .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
      .join("GP Connect")
      .join("gpservice.lock")
  }
}

#[cfg(windows)]
pub const SERVICE_PIPE: &str = r"\\.\pipe\GPConnect.Service";

#[cfg(windows)]
pub const SERVICE_NAME: &str = "GPConnect";

/// Installer-owned SYSTEM/Administrators-only state, including temporary keys.
#[cfg(windows)]
pub fn service_private_dir() -> PathBuf {
  service_lock_file()
    .parent()
    .expect("service state directory")
    .join("Private")
}
