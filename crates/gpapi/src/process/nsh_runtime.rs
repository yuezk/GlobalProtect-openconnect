//! Checks the generic Linux NSH runtime before offering a download.
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
pub(super) fn check() -> anyhow::Result<()> {
  check_gtk(OsStr::new("libgtk-4.so.1"))?;
  load_library(OsStr::new("libgnutls.so.30"), "GnuTLS 3")?;
  load_library(OsStr::new("libdbus-1.so.3"), "D-Bus")?;
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
