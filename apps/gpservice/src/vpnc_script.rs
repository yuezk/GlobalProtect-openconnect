use std::{
  fs::{self, File, Metadata, Permissions},
  io::{self, Read, Write},
  os::unix::fs::{MetadataExt, PermissionsExt},
  path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use common::constants::{INSTALLED_VPNC_SCRIPT, MAX_VPNC_SCRIPT_SIZE};
use tempfile::NamedTempFile;

const DIRECTORY_MODE: u32 = 0o755;
const SCRIPT_MODE: u32 = 0o755;
const ROOT_UID: u32 = 0;

pub fn installed_script() -> anyhow::Result<Option<PathBuf>> {
  let path = PathBuf::from(INSTALLED_VPNC_SCRIPT);
  match fs::symlink_metadata(&path) {
    Ok(metadata) => {
      validate_script_metadata(&path, &metadata, ROOT_UID)?;
      let directory = path.parent().context("Installed VPNC script has no parent directory")?;
      validate_directory(directory, ROOT_UID)?;
      validate_directory(
        directory
          .parent()
          .context("Installed VPNC script directory has no parent directory")?,
        ROOT_UID,
      )?;
      Ok(Some(path))
    }
    Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
    Err(err) => Err(err).context("Failed to inspect the installed VPNC script"),
  }
}

pub fn install(input: impl Read) -> anyhow::Result<()> {
  let target = Path::new(INSTALLED_VPNC_SCRIPT);
  let directory = target
    .parent()
    .context("Installed VPNC script has no parent directory")?;
  let state_directory = directory
    .parent()
    .context("Installed VPNC script directory has no parent directory")?;
  let state_parent = state_directory
    .parent()
    .context("VPNC script state directory has no parent directory")?;
  validate_directory(state_parent, ROOT_UID)?;
  ensure_directory(state_directory, ROOT_UID)?;
  ensure_directory(directory, ROOT_UID)?;
  install_at(input, target, ROOT_UID)
}

pub fn remove() -> anyhow::Result<()> {
  if installed_script()?.is_none() {
    return Ok(());
  }
  remove_at(Path::new(INSTALLED_VPNC_SCRIPT), ROOT_UID)
}

fn install_at(mut input: impl Read, target: &Path, owner_uid: u32) -> anyhow::Result<()> {
  let directory = target
    .parent()
    .context("Installed VPNC script has no parent directory")?;
  ensure_directory(directory, owner_uid)?;

  if let Ok(metadata) = fs::symlink_metadata(target) {
    validate_script_metadata(target, &metadata, owner_uid)?;
  }

  let mut contents = Vec::new();
  input
    .by_ref()
    .take(MAX_VPNC_SCRIPT_SIZE as u64 + 1)
    .read_to_end(&mut contents)
    .context("Failed to read the VPNC script")?;
  if contents.is_empty() {
    bail!("VPNC script is empty");
  }
  if contents.len() > MAX_VPNC_SCRIPT_SIZE {
    bail!("VPNC script exceeds the {} byte limit", MAX_VPNC_SCRIPT_SIZE);
  }

  let mut staged = NamedTempFile::new_in(directory).context("Failed to stage the VPNC script")?;
  staged
    .as_file()
    .set_permissions(Permissions::from_mode(SCRIPT_MODE))
    .context("Failed to set VPNC script permissions")?;
  staged.write_all(&contents).context("Failed to write the VPNC script")?;
  staged.as_file().sync_all().context("Failed to sync the VPNC script")?;
  staged
    .persist(target)
    .map_err(|err| err.error)
    .context("Failed to install the VPNC script")?;
  sync_directory(directory)?;

  let metadata = fs::symlink_metadata(target).context("Failed to inspect the installed VPNC script")?;
  validate_script_metadata(target, &metadata, owner_uid)
}

fn remove_at(target: &Path, owner_uid: u32) -> anyhow::Result<()> {
  let directory = target
    .parent()
    .context("Installed VPNC script has no parent directory")?;
  validate_directory(directory, owner_uid)?;

  match fs::symlink_metadata(target) {
    Ok(metadata) => {
      validate_script_metadata(target, &metadata, owner_uid)?;
      fs::remove_file(target).context("Failed to remove the installed VPNC script")?;
      sync_directory(directory)
    }
    Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
    Err(err) => Err(err).context("Failed to inspect the installed VPNC script"),
  }
}

fn ensure_directory(directory: &Path, owner_uid: u32) -> anyhow::Result<()> {
  match fs::symlink_metadata(directory) {
    Ok(_) => validate_directory(directory, owner_uid),
    Err(err) if err.kind() == io::ErrorKind::NotFound => {
      if let Some(parent) = directory.parent() {
        ensure_directory(parent, owner_uid)?;
      }
      fs::create_dir(directory).context("Failed to create the VPNC script directory")?;
      fs::set_permissions(directory, Permissions::from_mode(DIRECTORY_MODE))
        .context("Failed to set VPNC script directory permissions")?;
      validate_directory(directory, owner_uid)
    }
    Err(err) => Err(err).context("Failed to inspect the VPNC script directory"),
  }
}

fn validate_directory(path: &Path, owner_uid: u32) -> anyhow::Result<()> {
  let metadata = fs::symlink_metadata(path)
    .with_context(|| format!("Failed to inspect VPNC script directory {}", path.display()))?;
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    bail!("VPNC script directory {} is not a real directory", path.display());
  }
  validate_ownership_and_mode(path, &metadata, owner_uid)
}

fn validate_script_metadata(path: &Path, metadata: &Metadata, owner_uid: u32) -> anyhow::Result<()> {
  if metadata.file_type().is_symlink() || !metadata.is_file() {
    bail!("Installed VPNC script {} is not a regular file", path.display());
  }
  validate_ownership_and_mode(path, metadata, owner_uid)?;
  if metadata.mode() & 0o111 == 0 {
    bail!("Installed VPNC script {} is not executable", path.display());
  }
  Ok(())
}

fn validate_ownership_and_mode(path: &Path, metadata: &Metadata, owner_uid: u32) -> anyhow::Result<()> {
  if metadata.uid() != owner_uid {
    bail!("{} must be owned by uid {}", path.display(), owner_uid);
  }
  if metadata.mode() & 0o022 != 0 {
    bail!("{} must not be writable by group or other users", path.display());
  }
  Ok(())
}

fn sync_directory(directory: &Path) -> anyhow::Result<()> {
  File::open(directory)
    .and_then(|file| file.sync_all())
    .context("Failed to sync the VPNC script directory")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn installs_and_removes_script_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = fs::symlink_metadata(temp.path()).unwrap();
    let owner_uid = metadata.uid();
    let directory = temp.path().join("scripts");
    let target = directory.join("vpnc-script");

    install_at("#!/bin/sh\nexit 0\n".as_bytes(), &target, owner_uid).unwrap();
    let metadata = fs::symlink_metadata(&target).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.mode() & 0o777, SCRIPT_MODE);

    remove_at(&target, owner_uid).unwrap();
    assert!(!target.exists());
  }

  #[test]
  fn rejects_oversized_script() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = fs::symlink_metadata(temp.path()).unwrap();
    let owner_uid = metadata.uid();
    let target = temp.path().join("scripts/vpnc-script");
    let input = vec![b'x'; MAX_VPNC_SCRIPT_SIZE + 1];

    assert!(install_at(input.as_slice(), &target, owner_uid).is_err());
    assert!(!target.exists());
  }

  #[test]
  fn rejects_symlink_target() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let metadata = fs::symlink_metadata(temp.path()).unwrap();
    let owner_uid = metadata.uid();
    let directory = temp.path().join("scripts");
    fs::create_dir(&directory).unwrap();
    let target = directory.join("vpnc-script");
    symlink("/bin/sh", &target).unwrap();

    assert!(install_at("exit 0\n".as_bytes(), &target, owner_uid).is_err());
  }
}
