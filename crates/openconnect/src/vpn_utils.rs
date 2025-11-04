use std::{io, path::Path};

use is_executable::IsExecutable;

const VPNC_SCRIPT_LOCATIONS: &[&str] = &[
  "/usr/local/share/vpnc-scripts/vpnc-script",
  "/usr/local/sbin/vpnc-script",
  "/usr/share/vpnc-scripts/vpnc-script",
  "/usr/sbin/vpnc-script",
  "/etc/vpnc/vpnc-script",
  "/etc/openconnect/vpnc-script",
  "/usr/libexec/vpnc-scripts/vpnc-script",
  #[cfg(target_os = "macos")]
  "/opt/homebrew/etc/vpnc/vpnc-script",
];

const CSD_WRAPPER_LOCATIONS: &[&str] = &[
  #[cfg(target_arch = "x86_64")]
  "/usr/lib/x86_64-linux-gnu/openconnect/hipreport.sh",
  #[cfg(target_arch = "aarch64")]
  "/usr/lib/aarch64-linux-gnu/openconnect/hipreport.sh",
  "/usr/lib/openconnect/hipreport.sh",
  "/usr/libexec/openconnect/hipreport.sh",
  #[cfg(target_os = "macos")]
  "/opt/homebrew/opt/openconnect/libexec/openconnect/hipreport.sh",
];

fn find_executable(locations: &[&str]) -> Option<String> {
  for location in locations.iter() {
    let path = Path::new(location);
    if path.is_executable() {
      return Some(location.to_string());
    }
  }

  None
}

pub fn find_vpnc_script() -> Option<String> {
  find_executable(&VPNC_SCRIPT_LOCATIONS)
}

pub fn find_csd_wrapper() -> Option<String> {
  find_executable(&CSD_WRAPPER_LOCATIONS)
}

/// If file exists, check if it is executable
pub fn check_executable(file: &str) -> Result<(), io::Error> {
  let path = Path::new(file);

  if !path.is_executable() {
    return Err(io::Error::new(
      io::ErrorKind::PermissionDenied,
      format!("{} is not executable", file),
    ));
  }

  Ok(())
}
