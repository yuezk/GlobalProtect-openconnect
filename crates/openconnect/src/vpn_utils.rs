use std::{io, path::Path};

use is_executable::IsExecutable;

const VPNC_SCRIPT_LOCATIONS: &[&str] = &[
  "/usr/local/libexec/gpclient/vpnc-script",
  "/usr/libexec/gpclient/vpnc-script",
  "/usr/lib/gpclient/vpnc-script",
  "/usr/local/share/vpnc-scripts/vpnc-script",
  "/usr/local/sbin/vpnc-script",
  "/usr/share/vpnc-scripts/vpnc-script",
  "/usr/sbin/vpnc-script",
  "/etc/vpnc/vpnc-script",
  "/etc/openconnect/vpnc-script",
  "/usr/libexec/vpnc-scripts/vpnc-script",
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  "/opt/homebrew/etc/vpnc/vpnc-script",
  #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
  "/usr/local/etc/vpnc/vpnc-script",
];

fn find_executable(locations: &[&'static str]) -> Option<&'static str> {
  for location in locations.iter() {
    let path = Path::new(location);
    if path.is_executable() {
      return Some(*location);
    }
  }

  None
}

pub fn find_vpnc_script() -> Option<String> {
  if let Some(path) = std::env::var_os("GP_VPNC_SCRIPT").filter(|path| !path.is_empty()) {
    return configured_script(Path::new(&path));
  }
  find_executable(VPNC_SCRIPT_LOCATIONS).map(str::to_owned)
}

fn configured_script(path: &Path) -> Option<String> {
  if path.is_absolute() && path.is_file() && path.is_executable() {
    return path.to_str().map(str::to_owned);
  }
  None
}

/// If file exists, check if it is executable
pub fn check_executable(file: &str) -> Result<(), io::Error> {
  let path = Path::new(file);

  if path.exists() && !path.is_executable() {
    return Err(io::Error::new(
      io::ErrorKind::PermissionDenied,
      format!("{} is not executable", file),
    ));
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn configured_script_requires_an_absolute_executable_path() {
    assert_eq!(configured_script(Path::new("/bin/sh")), Some("/bin/sh".to_owned()));
    assert!(configured_script(Path::new("bin/sh")).is_none());
    assert!(configured_script(Path::new("/missing-openconnect-vpnc-script")).is_none());
    assert!(configured_script(Path::new("/etc/passwd")).is_none());
    assert!(configured_script(Path::new("/tmp")).is_none());
  }
}
