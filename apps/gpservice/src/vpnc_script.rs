use std::{
  fs::{self, File, Metadata, Permissions},
  io::{self, Read, Write},
  os::unix::fs::{MetadataExt, PermissionsExt},
  path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use common::constants::{INSTALLED_VPNC_SCRIPT, INSTALLED_VPNC_SCRIPT_METADATA, MAX_VPNC_SCRIPT_SIZE};
use gpapi::service::vpnc_script::{InstallVpncScriptRequest, VpncScriptMetadata};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

const DIRECTORY_MODE: u32 = 0o755;
const SCRIPT_MODE: u32 = 0o755;
const METADATA_MODE: u32 = 0o600;
const METADATA_VERSION: u8 = 1;
const MAX_METADATA_SIZE: u64 = 40 * 1024;
const ROOT_UID: u32 = 0;

#[derive(Deserialize, Serialize)]
struct StoredMetadata {
  version: u8,
  metadata: VpncScriptMetadata,
}

pub fn installed_script() -> anyhow::Result<Option<PathBuf>> {
  let path = PathBuf::from(INSTALLED_VPNC_SCRIPT);
  match fs::symlink_metadata(&path) {
    Ok(metadata) => {
      validate_script(&path, &metadata, ROOT_UID)?;
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

pub fn metadata() -> anyhow::Result<Option<VpncScriptMetadata>> {
  if installed_script()?.is_none() {
    return Ok(None);
  }
  read_metadata_at(Path::new(INSTALLED_VPNC_SCRIPT_METADATA), ROOT_UID)
}

pub fn install(request: InstallVpncScriptRequest) -> anyhow::Result<()> {
  request.metadata.validate()?;
  let contents = request.contents()?;
  let target = Path::new(INSTALLED_VPNC_SCRIPT);
  let metadata_target = Path::new(INSTALLED_VPNC_SCRIPT_METADATA);
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
  install_at(&contents, &request.metadata, target, metadata_target, ROOT_UID)
}

pub fn remove() -> anyhow::Result<()> {
  if installed_script()?.is_some() {
    remove_at(Path::new(INSTALLED_VPNC_SCRIPT), ROOT_UID, validate_script)?;
  }
  remove_at(Path::new(INSTALLED_VPNC_SCRIPT_METADATA), ROOT_UID, validate_metadata)
}

fn install_at(
  contents: &[u8],
  metadata: &VpncScriptMetadata,
  target: &Path,
  metadata_target: &Path,
  owner_uid: u32,
) -> anyhow::Result<()> {
  let directory = target
    .parent()
    .context("Installed VPNC script has no parent directory")?;
  ensure_directory(directory, owner_uid)?;

  if let Ok(existing) = fs::symlink_metadata(target) {
    validate_script(target, &existing, owner_uid)?;
  }
  if let Ok(existing) = fs::symlink_metadata(metadata_target) {
    validate_metadata(metadata_target, &existing, owner_uid)?;
  }
  if contents.is_empty() {
    bail!("VPNC script is empty");
  }
  if contents.len() > MAX_VPNC_SCRIPT_SIZE {
    bail!("VPNC script exceeds the {} byte limit", MAX_VPNC_SCRIPT_SIZE);
  }
  metadata.validate()?;

  let mut staged_script = NamedTempFile::new_in(directory).context("Failed to stage the VPNC script")?;
  staged_script
    .as_file()
    .set_permissions(Permissions::from_mode(SCRIPT_MODE))
    .context("Failed to set VPNC script permissions")?;
  staged_script
    .write_all(contents)
    .context("Failed to write the VPNC script")?;
  staged_script
    .as_file()
    .sync_all()
    .context("Failed to sync the VPNC script")?;

  let stored_metadata = StoredMetadata {
    version: METADATA_VERSION,
    metadata: metadata.clone(),
  };
  let mut staged_metadata = NamedTempFile::new_in(directory).context("Failed to stage VPNC script metadata")?;
  staged_metadata
    .as_file()
    .set_permissions(Permissions::from_mode(METADATA_MODE))
    .context("Failed to set VPNC script metadata permissions")?;
  serde_json::to_writer(&mut staged_metadata, &stored_metadata).context("Failed to write VPNC script metadata")?;
  staged_metadata
    .as_file()
    .sync_all()
    .context("Failed to sync VPNC script metadata")?;

  staged_script
    .persist(target)
    .map_err(|err| err.error)
    .context("Failed to install the VPNC script")?;
  staged_metadata
    .persist(metadata_target)
    .map_err(|err| err.error)
    .context("Failed to install VPNC script metadata")?;
  sync_directory(directory)?;

  let script_metadata = fs::symlink_metadata(target).context("Failed to inspect the installed VPNC script")?;
  validate_script(target, &script_metadata, owner_uid)?;
  let installed_metadata = fs::symlink_metadata(metadata_target).context("Failed to inspect VPNC script metadata")?;
  validate_metadata(metadata_target, &installed_metadata, owner_uid)
}

fn read_metadata_at(target: &Path, owner_uid: u32) -> anyhow::Result<Option<VpncScriptMetadata>> {
  let metadata = match fs::symlink_metadata(target) {
    Ok(metadata) => metadata,
    Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
    Err(err) => return Err(err).context("Failed to inspect VPNC script metadata"),
  };
  validate_metadata(target, &metadata, owner_uid)?;

  let file = File::open(target).context("Failed to open VPNC script metadata")?;
  let stored: StoredMetadata =
    serde_json::from_reader(file.take(MAX_METADATA_SIZE)).context("Failed to parse VPNC script metadata")?;
  if stored.version != METADATA_VERSION {
    bail!("Unsupported VPNC script metadata version {}", stored.version);
  }
  stored.metadata.validate()?;
  Ok(Some(stored.metadata))
}

fn remove_at(
  target: &Path,
  owner_uid: u32,
  validate: fn(&Path, &Metadata, u32) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
  let directory = target
    .parent()
    .context("VPNC script state file has no parent directory")?;
  validate_directory(directory, owner_uid)?;

  match fs::symlink_metadata(target) {
    Ok(metadata) => {
      validate(target, &metadata, owner_uid)?;
      fs::remove_file(target).context("Failed to remove VPNC script state")?;
      sync_directory(directory)
    }
    Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
    Err(err) => Err(err).context("Failed to inspect VPNC script state"),
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

fn validate_script(path: &Path, metadata: &Metadata, owner_uid: u32) -> anyhow::Result<()> {
  if metadata.file_type().is_symlink() || !metadata.is_file() {
    bail!("Installed VPNC script {} is not a regular file", path.display());
  }
  validate_ownership_and_mode(path, metadata, owner_uid)?;
  if metadata.mode() & 0o111 == 0 {
    bail!("Installed VPNC script {} is not executable", path.display());
  }
  Ok(())
}

fn validate_metadata(path: &Path, metadata: &Metadata, owner_uid: u32) -> anyhow::Result<()> {
  if metadata.file_type().is_symlink() || !metadata.is_file() {
    bail!("VPNC script metadata {} is not a regular file", path.display());
  }
  validate_ownership_and_mode(path, metadata, owner_uid)?;
  if metadata.mode() & 0o777 != METADATA_MODE {
    bail!("VPNC script metadata {} must have mode 0600", path.display());
  }
  if metadata.len() > MAX_METADATA_SIZE {
    bail!("VPNC script metadata {} is too large", path.display());
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
  use gpapi::service::vpnc_script::VpncScriptSource;

  use super::*;

  fn source(value: &str) -> VpncScriptMetadata {
    VpncScriptMetadata::new(VpncScriptSource::Command, value.to_owned()).unwrap()
  }

  #[test]
  fn installs_metadata_and_removes_both_files() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = fs::symlink_metadata(temp.path()).unwrap();
    let owner_uid = metadata.uid();
    let directory = temp.path().join("scripts");
    let target = directory.join("vpnc-script");
    let metadata_target = directory.join("vpnc-script.metadata");
    let source = source("exit 0");

    install_at(b"#!/bin/sh\nexit 0\n", &source, &target, &metadata_target, owner_uid).unwrap();
    let metadata = fs::symlink_metadata(&target).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.mode() & 0o777, SCRIPT_MODE);
    assert_eq!(read_metadata_at(&metadata_target, owner_uid).unwrap(), Some(source));

    remove_at(&target, owner_uid, validate_script).unwrap();
    remove_at(&metadata_target, owner_uid, validate_metadata).unwrap();
    assert!(!target.exists());
    assert!(!metadata_target.exists());
  }

  #[test]
  fn missing_metadata_is_allowed() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = fs::symlink_metadata(temp.path()).unwrap();

    assert_eq!(
      read_metadata_at(&temp.path().join("missing"), metadata.uid()).unwrap(),
      None
    );
  }

  #[test]
  fn rejects_oversized_script() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = fs::symlink_metadata(temp.path()).unwrap();
    let owner_uid = metadata.uid();
    let target = temp.path().join("scripts/vpnc-script");
    let metadata_target = temp.path().join("scripts/vpnc-script.metadata");
    let input = vec![b'x'; MAX_VPNC_SCRIPT_SIZE + 1];

    assert!(install_at(&input, &source("/tmp/script"), &target, &metadata_target, owner_uid).is_err());
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
    let metadata_target = directory.join("vpnc-script.metadata");
    symlink("/bin/sh", &target).unwrap();

    assert!(
      install_at(
        b"exit 0\n",
        &source("/tmp/script"),
        &target,
        &metadata_target,
        owner_uid
      )
      .is_err()
    );
  }
}
