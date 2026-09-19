//! Root-owned Unix installation identity. Failure affects GUI issuance only.
use anyhow::{Context, ensure};
use gpapi::device_anchor::{AnchorSource, DeviceAnchor};
use serde::{Deserialize, Serialize};
use std::{
  fs::{self, File, OpenOptions},
  io::{Read, Write},
  os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
  path::Path,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
  version: u32,
  anchor: DeviceAnchor,
}

pub fn resolve() -> anyhow::Result<DeviceAnchor> {
  resolve_in(Path::new("/var/lib/gp"), 0, gpapi::device_anchor::collect_unix)
    .context("cannot resolve persistent GUI device anchor")
}

fn validate_metadata(metadata: &fs::Metadata, owner: u32, directory: bool) -> anyhow::Result<()> {
  ensure!(
    if directory {
      metadata.is_dir()
    } else {
      metadata.is_file()
    },
    "unexpected device anchor state type"
  );
  ensure!(metadata.uid() == owner, "unexpected device anchor state owner");
  ensure!(
    metadata.permissions().mode() & 0o777 == if directory { 0o700 } else { 0o600 },
    "unsafe device anchor state permissions"
  );
  if !directory {
    ensure!(metadata.nlink() == 1, "device anchor state must not have hard links");
  }
  Ok(())
}

fn read_record(path: &Path, owner: u32) -> anyhow::Result<Option<DeviceAnchor>> {
  let mut file = match OpenOptions::new()
    .read(true)
    .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
    .open(path)
  {
    Ok(file) => file,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error.into()),
  };
  validate_metadata(&file.metadata()?, owner, false)?;
  ensure!(file.metadata()?.len() <= 4096, "oversized device anchor record");
  let mut bytes = Vec::new();
  file.read_to_end(&mut bytes)?;
  let record: Record = serde_json::from_slice(&bytes).context("malformed device anchor record")?;
  ensure!(
    record.version == 1 && record.anchor.validate(),
    "unsupported or invalid device anchor record"
  );
  ensure!(
    matches!(record.anchor.source, AnchorSource::LinuxMac | AnchorSource::LinuxRandom),
    "invalid Unix device anchor source"
  );
  Ok(Some(record.anchor))
}

fn resolve_in(
  directory: &Path,
  owner: u32,
  collect: impl FnOnce() -> anyhow::Result<DeviceAnchor>,
) -> anyhow::Result<DeviceAnchor> {
  match fs::DirBuilder::new().mode(0o700).create(directory) {
    Ok(()) => {
      if let Some(parent) = directory.parent() {
        File::open(parent)?.sync_all()?;
      }
    }
    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
    Err(error) => return Err(error.into()),
  }
  validate_metadata(&fs::symlink_metadata(directory)?, owner, true)?;
  let path = directory.join("hardware-id");
  let lock = OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .truncate(false)
    .mode(0o600)
    .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
    .open(directory.join("hardware-id.lock"))?;
  validate_metadata(&lock.metadata()?, owner, false)?;
  lock.lock()?;
  if let Some(anchor) = read_record(&path, owner)? {
    return Ok(anchor);
  }
  let anchor = collect()?;
  ensure!(
    anchor.validate() && matches!(anchor.source, AnchorSource::LinuxMac | AnchorSource::LinuxRandom),
    "invalid collected Unix device anchor"
  );
  let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
  temporary.as_file().set_permissions(fs::Permissions::from_mode(0o600))?;
  temporary.write_all(&serde_json::to_vec(&Record {
    version: 1,
    anchor: anchor.clone(),
  })?)?;
  temporary.as_file().sync_all()?;
  temporary.persist_noclobber(&path)?;
  File::open(directory)?.sync_all()?;
  Ok(anchor)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn anchor() -> anyhow::Result<DeviceAnchor> {
    Ok(DeviceAnchor {
      hardware_id: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
      source: AnchorSource::LinuxMac,
    })
  }

  #[test]
  fn persistence_reuses_the_existing_record_without_rediscovery() {
    let _ = resolve as fn() -> anyhow::Result<DeviceAnchor>;
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("state");
    let owner = nix::unistd::Uid::current().as_raw();
    assert_eq!(resolve_in(&path, owner, anchor).unwrap(), anchor().unwrap());
    assert_eq!(
      resolve_in(&path, owner, || anyhow::bail!("must not rediscover")).unwrap(),
      anchor().unwrap()
    );
  }
}
