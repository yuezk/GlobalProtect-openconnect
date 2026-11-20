//! Selects the Linux GUI using its installed or package-managed runtime.
//! Runs in the desktop supervisor, which does not load either GUI's GTK runtime.
use std::{ffi::OsStr, os::raw::c_uint};

use anyhow::{Context, ensure};
use libloading::{
  Library,
  os::unix::{RTLD_LOCAL, RTLD_NOW},
};

// Match nsh-platform-linux's gtk4/v4_14 feature when changing the NSH build.
const MIN_GTK_MINOR: u32 = 14;

#[cfg(target_os = "linux")]
pub(super) const TAURI_RECOVERY: &str = "To use Tauri, run: gpclient launch-gui --interface tauri";

#[cfg(target_os = "linux")]
pub(super) async fn select(
  requested: Option<crate::gui::GuiVariant>,
  service_input: &mut (impl tokio::io::AsyncWrite + Unpin),
  service_output: &mut (impl tokio::io::AsyncRead + Unpin),
) -> anyhow::Result<crate::gui::GuiVariant> {
  use crate::{gui::GuiVariant, service::transport::DESKTOP_GUI_INSTALLER};
  use tokio::io::{AsyncReadExt, AsyncWriteExt};

  if requested == Some(GuiVariant::Tauri) {
    return Ok(GuiVariant::Tauri);
  }

  let runtime = if crate::gui::NATIVE_SHELL_SUPPORTED {
    service_input.write_all(&[DESKTOP_GUI_INSTALLER]).await?;
    match service_output.read_u8().await? {
      0 => check(),
      // The package installer provides and wraps NSH's dependencies (e.g. NixOS).
      1 => Ok(()),
      _ => anyhow::bail!("Invalid GUI installer capability response"),
    }
  } else {
    Err(anyhow::anyhow!("Native Shell is unavailable for this platform."))
  };

  match runtime {
    Ok(()) => Ok(GuiVariant::Nsh),
    Err(err) if requested.is_some() => Err(err),
    Err(err) => {
      log::info!("Selecting Tauri: {err:#}");
      Ok(GuiVariant::Tauri)
    }
  }
}

#[cfg(target_os = "linux")]
pub(super) fn check() -> anyhow::Result<()> {
  check_gtk(OsStr::new("libgtk-4.so.1"))?;
  #[cfg(target_env = "gnu")]
  {
    // glibc returns a static, NUL-terminated runtime version string.
    let version = unsafe { std::ffi::CStr::from_ptr(nix::libc::gnu_get_libc_version()) };
    check_glibc_version(version.to_str()?)?;
  }
  load_library(OsStr::new("libgnutls.so.30"), "GnuTLS 3")?;
  load_library(OsStr::new("libdbus-1.so.3"), "D-Bus")?;
  Ok(())
}

#[cfg(any(all(target_os = "linux", target_env = "gnu"), test))]
fn check_glibc_version(version: &str) -> anyhow::Result<()> {
  let mut parts = version.split('.');
  let major = parts.next().and_then(|part| part.parse::<u32>().ok());
  let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
  ensure!(
    major.zip(minor).is_some_and(|version| version >= (2, 39)),
    "Native Shell requires glibc 2.39 or later. This system reports glibc {version}."
  );
  Ok(())
}

fn load_library(path: &OsStr, name: &str) -> anyhow::Result<Library> {
  // Resolve symbols now as well as transitive dependencies, without initializing GTK.
  let library = unsafe { libloading::os::unix::Library::open(Some(path), RTLD_LOCAL | RTLD_NOW) };
  library.map(Into::into).map_err(|error| {
    log::warn!("Native Shell runtime check failed for {name}: {error}");
    anyhow::anyhow!("Native Shell could not load {name}. Install its runtime libraries and try again.")
  })
}

fn check_gtk(path: &OsStr) -> anyhow::Result<()> {
  let library = load_library(path, "GTK 4.14 or later")?;
  type Version = unsafe extern "C" fn() -> c_uint;
  // These public GTK getters take no arguments and do not initialize a display.
  let (major, minor, micro) = unsafe {
    let major = library.get::<Version>(b"gtk_get_major_version\0");
    let minor = library.get::<Version>(b"gtk_get_minor_version\0");
    let micro = library.get::<Version>(b"gtk_get_micro_version\0");
    let message = "Native Shell requires GTK 4.14 or later. Update GTK 4 and try again.";
    (
      major.context(message)?(),
      minor.context(message)?(),
      micro.context(message)?(),
    )
  };
  ensure!(
    major == 4 && minor >= MIN_GTK_MINOR,
    "Native Shell requires GTK 4.14 or later. This system has GTK {major}.{minor}.{micro}."
  );
  Ok(())
}

#[cfg(test)]
pub(super) mod tests {
  use super::*;
  use std::{fs, process::Command};

  #[test]
  fn requires_glibc_2_39_or_later() {
    for version in ["2.9", "2.38", "unknown"] {
      assert!(
        check_glibc_version(version)
          .unwrap_err()
          .to_string()
          .contains("glibc 2.39")
      );
    }
    for version in ["2.39", "2.39.1", "2.40", "3.0"] {
      check_glibc_version(version).unwrap();
    }
  }

  pub(in crate::process) fn gtk_fixture(directory: &std::path::Path, minor: u32) -> std::path::PathBuf {
    let source = directory.join(format!("gtk-{minor}.c"));
    let library = directory.join(format!("gtk-{minor}.so"));
    fs::write(
      &source,
      format!(
        "unsigned gtk_get_major_version(void) {{ return 4; }}\n\
       unsigned gtk_get_minor_version(void) {{ return {minor}; }}\n\
       unsigned gtk_get_micro_version(void) {{ return 1; }}\n"
      ),
    )
    .unwrap();
    assert!(
      Command::new("cc")
        .args(["-shared", "-fPIC"])
        .arg(source)
        .arg("-o")
        .arg(&library)
        .status()
        .unwrap()
        .success()
    );
    library
  }

  #[test]
  fn loads_runtime_version_without_development_metadata_or_a_display() {
    let directory = tempfile::tempdir().unwrap();
    let old = gtk_fixture(directory.path(), 12);
    let supported = gtk_fixture(directory.path(), 14);
    let newer = gtk_fixture(directory.path(), 20);
    let error = check_gtk(old.as_os_str()).unwrap_err().to_string();
    assert!(error.contains("4.14"), "{error}");
    assert!(error.contains("4.12.1"), "{error}");
    check_gtk(supported.as_os_str()).unwrap();
    check_gtk(newer.as_os_str()).unwrap();
  }

  #[test]
  fn missing_runtime_reports_an_actionable_error() {
    let directory = tempfile::tempdir().unwrap();
    let error = check_gtk(directory.path().join("missing.so").as_os_str())
      .unwrap_err()
      .to_string();
    assert!(error.contains("GTK 4.14 or later"), "{error}");
    assert!(error.contains("Install"), "{error}");
  }
}
