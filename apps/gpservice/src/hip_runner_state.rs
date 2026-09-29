use std::{
  fs,
  os::unix::fs::{MetadataExt, PermissionsExt},
  path::Path,
};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

pub const RUNNER_NAME: &str = "gp-hip-runner";
pub const STATE_NAME: &str = "report.json";
pub const MAX_STATE_BYTES: usize = 512 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(tag = "source", rename_all = "camelCase")]
pub enum RunnerReport {
  Generated,
  Edited { xml: String },
  ApprovedRootScript { approval_id: String, owner_uid: u32 },
}

/// Check every component because a trusted file below a writable parent can
/// be replaced before a later HIP refresh.
pub fn validate_root_owned_path(path: &Path, file: bool) -> anyhow::Result<()> {
  ensure!(path.is_absolute(), "HIP runner path must be absolute");
  for component in path.ancestors().take_while(|part| part.as_os_str() != "/") {
    let metadata =
      fs::symlink_metadata(component).with_context(|| format!("Cannot inspect {}", component.display()))?;
    ensure!(!metadata.file_type().is_symlink(), "HIP runner path contains a symlink");
    ensure!(metadata.uid() == 0, "HIP runner path is not root-owned");
    ensure!(
      metadata.permissions().mode() & 0o022 == 0,
      "HIP runner path is writable by another user"
    );
    if component == path && file {
      ensure!(metadata.is_file(), "HIP runner is not a regular file");
      ensure!(
        metadata.permissions().mode() & 0o111 != 0,
        "HIP runner is not executable"
      );
    } else {
      ensure!(metadata.is_dir(), "HIP runner parent is not a directory");
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rejects_user_owned_runner_directory() {
    let temp = tempfile::tempdir().unwrap();
    if unsafe { nix::libc::geteuid() } != 0 {
      assert!(validate_root_owned_path(temp.path(), false).is_err());
    }
  }
}
