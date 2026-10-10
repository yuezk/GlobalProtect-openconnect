use std::{
  env,
  path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(all(
  not(debug_assertions),
  any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")
))]
use crate::constants::GP_DOWNLOADED_GUI_BINARY;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
use crate::constants::GP_HIP_SCRIPT_INSTALLER_BINARY;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
use crate::constants::GP_VPNC_SCRIPT_INSTALLER_BINARY;
use crate::constants::{GP_AUTH_BINARY, GP_CLIENT_BINARY, GP_GUI_BINARY, GP_GUI_HELPER_BINARY, GP_SERVICE_BINARY};

pub fn gpclient() -> PathBuf {
  resolve("GP_CLIENT_BINARY", "gpclient", GP_CLIENT_BINARY)
}

pub fn gpservice() -> PathBuf {
  resolve("GP_SERVICE_BINARY", "gpservice", GP_SERVICE_BINARY)
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub fn gp_vpnc_script_installer() -> PathBuf {
  resolve(
    "GP_VPNC_SCRIPT_INSTALLER_BINARY",
    "gp-vpnc-script-installer",
    GP_VPNC_SCRIPT_INSTALLER_BINARY,
  )
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub fn gp_hip_script_installer() -> PathBuf {
  resolve(
    "GP_HIP_SCRIPT_INSTALLER_BINARY",
    "gp-hip-script-installer",
    GP_HIP_SCRIPT_INSTALLER_BINARY,
  )
}

pub fn gpauth() -> PathBuf {
  resolve("GP_AUTH_BINARY", "gpauth", GP_AUTH_BINARY)
}

pub fn gpgui() -> PathBuf {
  resolve("GP_GUI_BINARY", "gpgui", GP_GUI_BINARY)
}

pub fn gpgui_update_target() -> PathBuf {
  #[cfg(all(
    not(debug_assertions),
    any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")
  ))]
  {
    PathBuf::from(GP_DOWNLOADED_GUI_BINARY)
  }

  #[cfg(any(
    debug_assertions,
    not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))
  ))]
  {
    gpgui()
  }
}

pub fn gpgui_helper() -> PathBuf {
  resolve("GP_GUI_HELPER_BINARY", "gpgui-helper", GP_GUI_HELPER_BINARY)
}

fn resolve(env_key: &str, binary_name: &str, default_path: &str) -> PathBuf {
  env::var_os(env_key)
    .filter(|value| !value.is_empty())
    .map(PathBuf::from)
    .or_else(|| sibling_binary(binary_name))
    .unwrap_or_else(|| PathBuf::from(default_path))
}

fn sibling_binary(binary_name: &str) -> Option<PathBuf> {
  let current_exe = env::current_exe().ok()?;
  sibling_binary_for_executable(&current_exe, binary_name)
}

fn sibling_binary_for_executable(executable: &Path, binary_name: &str) -> Option<PathBuf> {
  let executable = executable.canonicalize().ok()?;
  let bin_dir = executable.parent()?;
  let binary = bin_dir.join(binary_name);
  if is_file(&binary) {
    return Some(binary);
  }
  #[cfg(target_os = "macos")]
  if let Some(binary) = macos::helper_path(&executable, binary_name)
    && is_file(&binary)
  {
    return Some(binary);
  }
  None
}

#[cfg(target_os = "macos")]
pub fn bundled_vpnc_script() -> Option<PathBuf> {
  bundled_vpnc_script_for_executable(&env::current_exe().ok()?)
}

#[cfg(target_os = "macos")]
fn bundled_vpnc_script_for_executable(executable: &Path) -> Option<PathBuf> {
  let executable = executable.canonicalize().ok()?;
  macos::vpnc_script_path(&executable)
}

fn is_file(path: &Path) -> bool {
  path.is_file()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  #[cfg(target_os = "macos")]
  fn desktop_resolves_helpers_in_the_actual_app_bundle_layout() {
    assert_eq!(
      macos::helper_path(
        Path::new("/Applications/GP Connect.app/Contents/MacOS/gpgui"),
        "gpclient"
      ),
      Some(PathBuf::from("/Applications/GP Connect.app/Contents/Helpers/gpclient"))
    );
    assert!(macos::helper_path(Path::new("/usr/local/bin/gpgui"), "gpclient").is_none());
    assert!(macos::helper_path(Path::new("/tmp/Contents/MacOS/gpgui"), "gpclient").is_none());
  }

  #[test]
  #[cfg(unix)]
  fn symlinked_executable_resolves_its_bundle_paths() {
    let directory = tempfile::tempdir().unwrap();
    let helpers = directory.path().join("GP Connect.app/Contents/Helpers");
    let bin = directory.path().join("bin");
    std::fs::create_dir_all(&helpers).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(helpers.join("gpclient"), "").unwrap();
    std::fs::write(helpers.join("gpauth"), "").unwrap();
    std::os::unix::fs::symlink(helpers.join("gpclient"), bin.join("gpclient")).unwrap();

    assert_eq!(
      sibling_binary_for_executable(&bin.join("gpclient"), "gpauth"),
      Some(helpers.canonicalize().unwrap().join("gpauth"))
    );
    #[cfg(target_os = "macos")]
    assert_eq!(
      bundled_vpnc_script_for_executable(&bin.join("gpclient")),
      Some(
        helpers
          .canonicalize()
          .unwrap()
          .parent()
          .unwrap()
          .join("Resources/Scripts/vpnc-script")
      )
    );
  }

  #[test]
  fn default_path_is_used_when_no_sibling_exists() {
    let path = resolve("GP_TEST_BINARY", "missing-gp-test-binary", "/usr/bin/gp-test");

    assert_eq!(path, PathBuf::from("/usr/bin/gp-test"));
  }
}
