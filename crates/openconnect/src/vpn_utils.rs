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
  if let Ok(executable) = std::env::current_exe()
    && let Some(script) = installed_vpnc_script(&executable)
  {
    return Some(script);
  }
  find_executable(VPNC_SCRIPT_LOCATIONS).map(str::to_owned)
}

fn installed_vpnc_script(executable: &Path) -> Option<String> {
  let executable = executable.canonicalize().ok()?;
  let bin = executable.parent()?;
  if bin.file_name()? != "bin" {
    return None;
  }
  configured_script(&bin.parent()?.join("libexec/gpclient/vpnc-script"))
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
  fn installed_script_follows_the_executable_prefix_through_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("prefix/bin");
    let libexec = dir.path().join("prefix/libexec/gpclient");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&libexec).unwrap();
    std::fs::write(bin.join("gpclient"), "").unwrap();
    let script = libexec.join("vpnc-script");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let link = dir.path().join("gpclient-link");
    std::os::unix::fs::symlink(bin.join("gpclient"), &link).unwrap();
    assert_eq!(
      installed_vpnc_script(&link),
      Some(script.canonicalize().unwrap().to_str().unwrap().to_owned())
    );
  }

  #[test]
  fn configured_script_requires_an_absolute_executable_path() {
    assert_eq!(configured_script(Path::new("/bin/sh")), Some("/bin/sh".to_owned()));
    assert!(configured_script(Path::new("bin/sh")).is_none());
    assert!(configured_script(Path::new("/missing-openconnect-vpnc-script")).is_none());
    assert!(configured_script(Path::new("/etc/passwd")).is_none());
    assert!(configured_script(Path::new("/tmp")).is_none());
  }
}
