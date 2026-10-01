use std::{
  fs::{self, File, OpenOptions},
  io::{Read, Write},
  os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
  path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use gpapi::hip::HipApprovalStatus;
use serde::{Deserialize, Serialize};
use tempfile::Builder;
use uuid::Uuid;

#[cfg(target_os = "linux")]
pub const APPROVAL_DIRECTORY: &str = "/var/lib/gpclient/hip-approvals";
#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
pub const APPROVAL_DIRECTORY: &str = "/var/db/gpclient/hip-approvals";
pub const MAX_SCRIPT_SIZE: usize = common::constants::MAX_HIP_SCRIPT_SIZE;
const MAX_METADATA_SIZE: u64 = 8 * 1024;
const METADATA_VERSION: u8 = 1;
const SCRIPT_NAME: &str = "script";
const METADATA_NAME: &str = "metadata.json";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
  pub original_path: String,
  pub contents_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalInfo {
  pub approval_id: String,
  pub owner_uid: u32,
  pub sha256: String,
  pub original_path: String,
}

#[derive(Clone, Debug)]
pub struct ApprovedScript {
  pub info: ApprovalInfo,
  pub path: PathBuf,
}

#[derive(Deserialize, Serialize)]
struct StoredMetadata {
  version: u8,
  info: ApprovalInfo,
}

impl InstallRequest {
  fn contents(&self) -> anyhow::Result<Vec<u8>> {
    ensure!(self.original_path.len() <= 4096, "HIP script path is too long");
    ensure!(
      Path::new(&self.original_path).is_absolute(),
      "HIP script path must be absolute"
    );
    ensure!(
      !self.original_path.contains('\0'),
      "HIP script path contains a NUL byte"
    );
    ensure!(
      self.contents_base64.len() <= (MAX_SCRIPT_SIZE + 2).div_ceil(3) * 4,
      "HIP script exceeds 1 MiB"
    );
    let contents = STANDARD
      .decode(&self.contents_base64)
      .context("Invalid HIP script content encoding")?;
    ensure!(
      !contents.is_empty() && contents.len() <= MAX_SCRIPT_SIZE,
      "HIP script must be 1 MiB or smaller"
    );
    Ok(contents)
  }
}

pub fn install(request: InstallRequest, approving_uid: u32) -> anyhow::Result<ApprovalInfo> {
  ensure!(approving_uid != 0, "Root cannot approve a desktop HIP script");
  let contents = request.contents()?;
  let base = Path::new(APPROVAL_DIRECTORY);
  fs::create_dir_all(base).context("Cannot create HIP approval directory")?;
  validate_approval_ancestors(base)?;
  fs::set_permissions(base, fs::Permissions::from_mode(0o700))?;
  install_at(base, &contents, request.original_path, approving_uid, 0)
}

pub fn resolve(approval_id: &str, desktop_uid: u32) -> anyhow::Result<ApprovedScript> {
  let budget = gpapi::process::collection::CollectionBudget::new(std::time::Duration::from_secs(60));
  resolve_with_control(approval_id, desktop_uid, &budget)
}

pub fn resolve_with_control(
  approval_id: &str,
  desktop_uid: u32,
  control: &dyn gpapi::process::collection::CollectionControl,
) -> anyhow::Result<ApprovedScript> {
  control.check()?;
  ensure!(desktop_uid != 0, "Root is not a desktop HIP approval owner");
  let base = Path::new(APPROVAL_DIRECTORY);
  validate_approval_ancestors(base)?;
  resolve_at_with_control(base, approval_id, desktop_uid, 0, control)
}

pub fn revoke(approval_id: &str, approving_uid: u32) -> anyhow::Result<()> {
  ensure!(approving_uid != 0, "Root cannot revoke another user's HIP approval");
  let base = Path::new(APPROVAL_DIRECTORY);
  validate_approval_ancestors(base)?;
  revoke_at(base, approval_id, approving_uid, 0)
}

pub fn status(approval_id: &str, desktop_uid: u32) -> anyhow::Result<HipApprovalStatus> {
  status_at(Path::new(APPROVAL_DIRECTORY), approval_id, desktop_uid, 0, true)
}

fn status_at(
  base: &Path,
  approval_id: &str,
  desktop_uid: u32,
  file_owner: u32,
  validate_root_ancestors: bool,
) -> anyhow::Result<HipApprovalStatus> {
  ensure!(desktop_uid != 0, "HIP approval status requires a desktop user");
  validate_id(approval_id)?;
  for path in [
    base.to_path_buf(),
    base.join(approval_id),
    base.join(approval_id).join(METADATA_NAME),
  ] {
    match fs::symlink_metadata(&path) {
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(HipApprovalStatus::Revoked),
      Err(_) => return Ok(HipApprovalStatus::Corrupt),
      Ok(_) => {}
    }
  }
  if validate_private_dir(base, file_owner).is_err()
    || (validate_root_ancestors && validate_approval_ancestors(base).is_err())
  {
    return Ok(HipApprovalStatus::Corrupt);
  }
  let info = match read_metadata_unbound_at(base, approval_id, file_owner) {
    Ok(info) => info,
    Err(_) => return Ok(HipApprovalStatus::Corrupt),
  };
  if info.owner_uid != desktop_uid {
    return Ok(HipApprovalStatus::OtherUser);
  }
  Ok(if resolve_at(base, approval_id, desktop_uid, file_owner).is_ok() {
    HipApprovalStatus::Valid
  } else {
    HipApprovalStatus::Corrupt
  })
}

fn install_at(
  base: &Path,
  contents: &[u8],
  original_path: String,
  approving_uid: u32,
  file_owner: u32,
) -> anyhow::Result<ApprovalInfo> {
  validate_private_dir(base, file_owner)?;
  ensure!(
    !contents.is_empty() && contents.len() <= MAX_SCRIPT_SIZE,
    "HIP script must be 1 MiB or smaller"
  );
  let approval_id = Uuid::new_v4().to_string();
  let info = ApprovalInfo {
    approval_id: approval_id.clone(),
    owner_uid: approving_uid,
    sha256: sha256::digest(contents),
    original_path,
  };
  let staged = Builder::new().prefix(".install-").tempdir_in(base)?;
  fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o700))?;
  validate_private_dir(staged.path(), file_owner)?;

  let script = staged.path().join(SCRIPT_NAME);
  write_new(&script, contents, 0o500)?;
  let metadata = staged.path().join(METADATA_NAME);
  let stored = StoredMetadata {
    version: METADATA_VERSION,
    info: info.clone(),
  };
  let encoded = serde_json::to_vec(&stored)?;
  ensure!(
    encoded.len() as u64 <= MAX_METADATA_SIZE,
    "HIP approval metadata is too large"
  );
  write_new(&metadata, &encoded, 0o600)?;
  File::open(staged.path())?.sync_all()?;

  let target = base.join(&approval_id);
  ensure!(!target.exists(), "HIP approval ID collision");
  fs::rename(staged.path(), &target).context("Cannot publish HIP approval")?;
  File::open(base)?.sync_all()?;
  Ok(info)
}

fn resolve_at(base: &Path, approval_id: &str, desktop_uid: u32, file_owner: u32) -> anyhow::Result<ApprovedScript> {
  let budget = gpapi::process::collection::CollectionBudget::new(std::time::Duration::from_secs(60));
  resolve_at_with_control(base, approval_id, desktop_uid, file_owner, &budget)
}

fn resolve_at_with_control(
  base: &Path,
  approval_id: &str,
  desktop_uid: u32,
  file_owner: u32,
  control: &dyn gpapi::process::collection::CollectionControl,
) -> anyhow::Result<ApprovedScript> {
  control.check()?;
  let info = read_metadata_at(base, approval_id, desktop_uid, file_owner)?;
  control.check()?;
  let script = base.join(approval_id).join(SCRIPT_NAME);
  validate_file(&script, file_owner, 0o500, MAX_SCRIPT_SIZE as u64)?;
  let mut file = File::open(&script)?;
  let mut contents = Vec::new();
  let mut chunk = [0_u8; 8192];
  loop {
    control.check()?;
    let read = file.read(&mut chunk)?;
    if read == 0 {
      break;
    }
    ensure!(
      contents.len() + read <= MAX_SCRIPT_SIZE,
      "Approved HIP script is too large"
    );
    contents.extend_from_slice(&chunk[..read]);
  }
  ensure!(!contents.is_empty(), "Approved HIP script is empty");
  ensure!(
    sha256::digest(&contents) == info.sha256,
    "Approved HIP script digest mismatch"
  );
  control.check()?;
  Ok(ApprovedScript { info, path: script })
}

fn read_metadata_at(base: &Path, approval_id: &str, desktop_uid: u32, file_owner: u32) -> anyhow::Result<ApprovalInfo> {
  let info = read_metadata_unbound_at(base, approval_id, file_owner)?;
  ensure!(info.owner_uid == desktop_uid, "HIP approval belongs to another user");
  Ok(info)
}

fn read_metadata_unbound_at(base: &Path, approval_id: &str, file_owner: u32) -> anyhow::Result<ApprovalInfo> {
  validate_id(approval_id)?;
  validate_private_dir(base, file_owner)?;
  let directory = base.join(approval_id);
  validate_private_dir(&directory, file_owner)?;
  let metadata_path = directory.join(METADATA_NAME);
  validate_file(&metadata_path, file_owner, 0o600, MAX_METADATA_SIZE)?;
  let stored: StoredMetadata = serde_json::from_slice(&fs::read(metadata_path)?)?;
  ensure!(stored.version == METADATA_VERSION, "Unsupported HIP approval version");
  ensure!(stored.info.approval_id == approval_id, "HIP approval ID mismatch");
  ensure!(
    stored.info.sha256.len() == 64 && stored.info.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
    "Invalid HIP approval digest"
  );

  Ok(stored.info)
}

fn revoke_at(base: &Path, approval_id: &str, approving_uid: u32, file_owner: u32) -> anyhow::Result<()> {
  validate_id(approval_id)?;
  let directory = base.join(approval_id);
  // The GUI may retry after removal succeeds but saving its settings fails.
  if let Err(err) = fs::symlink_metadata(&directory) {
    if err.kind() == std::io::ErrorKind::NotFound {
      return Ok(());
    }
    return Err(err).context("Cannot inspect HIP approval directory");
  }
  read_metadata_at(base, approval_id, approving_uid, file_owner)?;
  // Renaming first makes revocation atomic for the runner. Cleanup is separate:
  // a failed cleanup cannot make this approval executable again.
  let revoked = base.join(format!(".revoked-{approval_id}"));
  fs::rename(&directory, &revoked).context("Cannot revoke HIP approval")?;
  File::open(base)?.sync_all()?;
  if let Err(err) = fs::remove_dir_all(&revoked) {
    log::warn!("HIP approval was revoked but its snapshot could not be removed: {err}");
  } else {
    File::open(base)?.sync_all()?;
  }
  Ok(())
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> anyhow::Result<()> {
  let mut file = OpenOptions::new().write(true).create_new(true).mode(mode).open(path)?;
  file.write_all(bytes)?;
  file.sync_all()?;
  Ok(())
}

fn validate_id(value: &str) -> anyhow::Result<()> {
  let parsed = Uuid::parse_str(value).context("Invalid HIP approval ID")?;
  ensure!(parsed.to_string() == value, "HIP approval ID must use canonical form");
  Ok(())
}

fn validate_private_dir(path: &Path, owner_uid: u32) -> anyhow::Result<()> {
  let metadata = fs::symlink_metadata(path).with_context(|| format!("Cannot inspect {}", path.display()))?;
  ensure!(
    metadata.is_dir() && !metadata.file_type().is_symlink(),
    "HIP approval directory is not a directory"
  );
  ensure!(
    metadata.uid() == owner_uid && metadata.permissions().mode() & 0o077 == 0,
    "HIP approval directory {} is not private (owner {}, mode {:o})",
    path.display(),
    metadata.uid(),
    metadata.permissions().mode() & 0o777
  );
  Ok(())
}

fn validate_file(path: &Path, owner_uid: u32, mode: u32, max_size: u64) -> anyhow::Result<()> {
  let metadata = fs::symlink_metadata(path).with_context(|| format!("Cannot inspect {}", path.display()))?;
  ensure!(
    metadata.is_file() && !metadata.file_type().is_symlink(),
    "HIP approval file is not regular"
  );
  ensure!(
    metadata.uid() == owner_uid && metadata.permissions().mode() & 0o777 == mode,
    "HIP approval file has unsafe ownership or mode"
  );
  ensure!(metadata.len() <= max_size, "HIP approval file is too large");
  Ok(())
}

/// Check every component because a trusted file below a writable parent can
/// be replaced before a later HIP refresh.
fn validate_approval_ancestors(path: &Path) -> anyhow::Result<()> {
  ensure!(path.is_absolute(), "HIP approval path must be absolute");
  for component in path.ancestors().take_while(|part| part.as_os_str() != "/") {
    let metadata =
      fs::symlink_metadata(component).with_context(|| format!("Cannot inspect {}", component.display()))?;
    ensure!(
      !metadata.file_type().is_symlink(),
      "HIP approval path contains a symlink"
    );
    ensure!(metadata.uid() == 0, "HIP approval path is not root-owned");
    ensure!(
      metadata.permissions().mode() & 0o022 == 0,
      "HIP approval path is writable by another user"
    );
    ensure!(metadata.is_dir(), "HIP approval parent is not a directory");
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn fixture() -> (tempfile::TempDir, u32) {
    let base = tempfile::tempdir().unwrap();
    fs::set_permissions(base.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let owner = fs::symlink_metadata(base.path()).unwrap().uid();
    (base, owner)
  }

  #[test]
  fn install_request_rejects_oversized_or_relative_script() {
    let request = InstallRequest {
      original_path: "hip.sh".into(),
      contents_base64: STANDARD.encode(b"#!/bin/sh\n"),
    };
    assert!(request.contents().is_err());
    let oversized = InstallRequest {
      original_path: "/home/user/hip.sh".into(),
      contents_base64: STANDARD.encode(vec![b'x'; MAX_SCRIPT_SIZE + 1]),
    };
    assert!(oversized.contents().is_err());
  }

  #[test]
  fn approval_binds_owner_and_exact_script_digest() {
    let (base, owner) = fixture();
    let info = install_at(
      base.path(),
      b"#!/bin/sh\necho ok\n",
      "/home/user/hip.sh".into(),
      1000,
      owner,
    )
    .unwrap();
    let approved = resolve_at(base.path(), &info.approval_id, 1000, owner).unwrap();
    assert_eq!(approved.info.sha256, info.sha256);
    assert!(resolve_at(base.path(), &info.approval_id, 1001, owner).is_err());
    fs::set_permissions(&approved.path, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(&approved.path, b"#!/bin/sh\necho changed\n").unwrap();
    fs::set_permissions(&approved.path, fs::Permissions::from_mode(0o500)).unwrap();
    assert!(resolve_at(base.path(), &info.approval_id, 1000, owner).is_err());
  }

  #[test]
  fn revoked_approval_is_unavailable() {
    let (base, owner) = fixture();
    let info = install_at(base.path(), b"#!/bin/sh\n", "/home/user/hip.sh".into(), 1000, owner).unwrap();
    assert!(revoke_at(base.path(), &info.approval_id, 1001, owner).is_err());
    revoke_at(base.path(), &info.approval_id, 1000, owner).unwrap();
    revoke_at(base.path(), &info.approval_id, 1000, owner).unwrap();
    assert!(resolve_at(base.path(), &info.approval_id, 1000, owner).is_err());
  }

  #[test]
  fn status_distinguishes_owner_corruption_and_revocation() {
    let (base, owner) = fixture();
    let info = install_at(
      base.path(),
      b"#!/bin/sh\necho ok\n",
      "/home/user/hip.sh".into(),
      1000,
      owner,
    )
    .unwrap();
    assert_eq!(
      status_at(base.path(), &info.approval_id, 1000, owner, false).unwrap(),
      HipApprovalStatus::Valid
    );
    assert_eq!(
      status_at(base.path(), &info.approval_id, 1001, owner, false).unwrap(),
      HipApprovalStatus::OtherUser
    );
    let script = base.path().join(&info.approval_id).join(SCRIPT_NAME);
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
      status_at(base.path(), &info.approval_id, 1000, owner, false).unwrap(),
      HipApprovalStatus::Corrupt
    );
    fs::remove_file(base.path().join(&info.approval_id).join(METADATA_NAME)).unwrap();
    assert_eq!(
      status_at(base.path(), &info.approval_id, 1000, owner, false).unwrap(),
      HipApprovalStatus::Revoked
    );
  }

  #[test]
  fn revoke_succeeds_if_script_was_corrupted() {
    let (base, owner) = fixture();
    let info = install_at(base.path(), b"#!/bin/sh\n", "/home/user/hip.sh".into(), 1000, owner).unwrap();
    let script = base.path().join(&info.approval_id).join(SCRIPT_NAME);
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(&script, b"changed").unwrap();
    assert!(resolve_at(base.path(), &info.approval_id, 1000, owner).is_err());
    revoke_at(base.path(), &info.approval_id, 1000, owner).unwrap();
    assert!(!base.path().join(&info.approval_id).exists());
  }

  #[test]
  fn rejects_symlinked_script() {
    use std::os::unix::fs::symlink;
    let (base, owner) = fixture();
    let info = install_at(base.path(), b"#!/bin/sh\n", "/home/user/hip.sh".into(), 1000, owner).unwrap();
    let script = base.path().join(&info.approval_id).join(SCRIPT_NAME);
    fs::remove_file(&script).unwrap();
    symlink("/bin/sh", &script).unwrap();
    assert!(resolve_at(base.path(), &info.approval_id, 1000, owner).is_err());
  }
}
