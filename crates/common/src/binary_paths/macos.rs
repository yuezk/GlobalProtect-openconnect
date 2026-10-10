use std::path::{Path, PathBuf};

fn contents_dir(executable: &Path) -> Option<&Path> {
  let bin_dir = executable.parent()?;
  let contents = bin_dir.parent()?;
  let bundle = contents.parent()?;
  let directory_name = bin_dir.file_name()?;
  if (directory_name != "MacOS" && directory_name != "Helpers")
    || contents.file_name()? != "Contents"
    || bundle.extension()? != "app"
  {
    return None;
  }
  Some(contents)
}

pub(super) fn helper_path(executable: &Path, binary_name: &str) -> Option<PathBuf> {
  Some(contents_dir(executable)?.join("Helpers").join(binary_name))
}

pub(super) fn vpnc_script_path(executable: &Path) -> Option<PathBuf> {
  Some(contents_dir(executable)?.join("Resources/Scripts/vpnc-script"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn bundle_entry_points_use_their_own_vpnc_script() {
    for executable in [
      "/Applications/GP Connect.app/Contents/Helpers/gpclient",
      "/Applications/GP Connect.app/Contents/MacOS/gpgui",
    ] {
      assert_eq!(
        vpnc_script_path(Path::new(executable)),
        Some(PathBuf::from(
          "/Applications/GP Connect.app/Contents/Resources/Scripts/vpnc-script"
        ))
      );
    }
    assert!(vpnc_script_path(Path::new("/opt/homebrew/bin/gpclient")).is_none());
    assert!(vpnc_script_path(Path::new("/tmp/Contents/Helpers/gpclient")).is_none());
  }
}
