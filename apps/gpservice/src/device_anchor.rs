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
  resolve_in(Path::new("/var/lib/gp"), 0, collect_unix).context("cannot resolve persistent GUI device anchor")
}

fn collect_unix() -> anyhow::Result<DeviceAnchor> {
  collect_from(mac_address::get_mac_address()?, || {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("cannot initialize device anchor"))?;
    Ok(random)
  })
}

fn collect_from(
  mac: Option<mac_address::MacAddress>,
  random: impl FnOnce() -> anyhow::Result<[u8; 32]>,
) -> anyhow::Result<DeviceAnchor> {
  if let Some(mac) = mac {
    let bytes = mac.bytes();
    if bytes != [0; 6] && bytes[0] & 1 == 0 {
      return Ok(DeviceAnchor::derive(AnchorSource::UnixMac, bytes));
    }
  }
  Ok(DeviceAnchor::derive(AnchorSource::UnixRandom, random()?))
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
    matches!(record.anchor.source, AnchorSource::UnixMac | AnchorSource::UnixRandom),
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
    anchor.validate() && matches!(anchor.source, AnchorSource::UnixMac | AnchorSource::UnixRandom),
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
      source: AnchorSource::UnixMac,
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

  #[test]
  fn collector_preserves_an_eligible_mac_and_randomizes_ineligible_addresses() {
    use mac_address::MacAddress;

    let eligible = collect_from(Some(MacAddress::new([0, 17, 34, 51, 68, 85])), || {
      anyhow::bail!("must not randomize")
    })
    .unwrap();
    assert_eq!(
      eligible,
      DeviceAnchor::derive(AnchorSource::UnixMac, [0, 17, 34, 51, 68, 85])
    );

    for rejected in [[0; 6], [1, 2, 3, 4, 5, 6], [0xff; 6]] {
      assert_eq!(
        collect_from(Some(MacAddress::new(rejected)), || Ok([9; 32])).unwrap(),
        DeviceAnchor::derive(AnchorSource::UnixRandom, [9; 32])
      );
    }
    assert_eq!(
      collect_from(None, || Ok([8; 32])).unwrap(),
      DeviceAnchor::derive(AnchorSource::UnixRandom, [8; 32])
    );
  }

  #[test]
  fn deleting_the_record_recreates_the_same_mac_anchor() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("state");
    let owner = nix::unistd::Uid::current().as_raw();
    let first = resolve_in(&path, owner, anchor).unwrap();
    fs::remove_file(path.join("hardware-id")).unwrap();
    assert_eq!(resolve_in(&path, owner, anchor).unwrap(), first);
  }

  #[test]
  fn parallel_first_start_converges_on_one_record() {
    use std::sync::{Arc, Barrier};

    let temporary = tempfile::tempdir().unwrap();
    let path = Arc::new(temporary.path().join("state"));
    fs::create_dir(&*path).unwrap();
    fs::set_permissions(&*path, fs::Permissions::from_mode(0o700)).unwrap();
    let owner = nix::unistd::Uid::current().as_raw();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = [AnchorSource::UnixMac, AnchorSource::UnixRandom]
      .into_iter()
      .map(|source| {
        let path = Arc::clone(&path);
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
          barrier.wait();
          resolve_in(&path, owner, || Ok(DeviceAnchor::derive(source, [source.into()])))
        })
      })
      .collect();
    let results: Vec<_> = handles
      .into_iter()
      .map(|handle| handle.join().unwrap().unwrap())
      .collect();
    assert_eq!(results[0], results[1]);
    assert_eq!(
      read_record(&path.join("hardware-id"), owner).unwrap(),
      Some(results[0].clone())
    );
  }

  #[test]
  fn rejects_malformed_linked_and_unsafe_records() {
    use std::os::unix::fs::symlink;

    let owner = nix::unistd::Uid::current().as_raw();
    for case in ["malformed", "symlink", "hard-link", "permissions"] {
      let temporary = tempfile::tempdir().unwrap();
      let directory = temporary.path().join("state");
      fs::create_dir(&directory).unwrap();
      fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
      let record = directory.join("hardware-id");
      match case {
        "malformed" => fs::write(&record, b"not-json").unwrap(),
        "symlink" => {
          let target = temporary.path().join("target");
          fs::write(
            &target,
            serde_json::to_vec(&Record {
              version: 1,
              anchor: anchor().unwrap(),
            })
            .unwrap(),
          )
          .unwrap();
          symlink(target, &record).unwrap();
        }
        "hard-link" => {
          fs::write(
            &record,
            serde_json::to_vec(&Record {
              version: 1,
              anchor: anchor().unwrap(),
            })
            .unwrap(),
          )
          .unwrap();
          fs::hard_link(&record, temporary.path().join("second-link")).unwrap();
        }
        "permissions" => {
          fs::write(
            &record,
            serde_json::to_vec(&Record {
              version: 1,
              anchor: anchor().unwrap(),
            })
            .unwrap(),
          )
          .unwrap();
          fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).unwrap();
        }
        _ => unreachable!(),
      }
      if case != "symlink" && case != "permissions" {
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
      }
      assert!(resolve_in(&directory, owner, anchor).is_err(), "accepted {case}");
    }
  }

  #[test]
  fn recovers_with_an_interrupted_temporary_file_present() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("state");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(directory.join(".hardware-id.interrupted"), b"partial").unwrap();
    assert_eq!(
      resolve_in(&directory, nix::unistd::Uid::current().as_raw(), anchor).unwrap(),
      anchor().unwrap()
    );
  }

  #[test]
  fn rejects_unsafe_directory_and_lock_metadata_and_read_only_storage() {
    use std::os::unix::fs::symlink;

    let owner = nix::unistd::Uid::current().as_raw();

    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("state");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(resolve_in(&directory, owner, anchor).is_err());

    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("state");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(resolve_in(&directory, owner.wrapping_add(1), anchor).is_err());

    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("state");
    fs::write(&directory, b"not-a-directory").unwrap();
    assert!(resolve_in(&directory, owner, anchor).is_err());

    for case in ["symlink", "hard-link", "permissions"] {
      let temporary = tempfile::tempdir().unwrap();
      let directory = temporary.path().join("state");
      fs::create_dir(&directory).unwrap();
      fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
      let lock = directory.join("hardware-id.lock");
      match case {
        "symlink" => {
          let target = temporary.path().join("target");
          fs::write(&target, b"").unwrap();
          symlink(target, &lock).unwrap();
        }
        "hard-link" => {
          fs::write(&lock, b"").unwrap();
          fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
          fs::hard_link(&lock, temporary.path().join("second-link")).unwrap();
        }
        "permissions" => {
          fs::write(&lock, b"").unwrap();
          fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
        }
        _ => unreachable!(),
      }
      assert!(resolve_in(&directory, owner, anchor).is_err(), "accepted {case} lock");
    }

    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("state");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o500)).unwrap();
    assert!(resolve_in(&directory, owner, anchor).is_err());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
  }

  #[test]
  fn record_serializes_source_as_a_number_and_rejects_unknown_codes() {
    let record = Record {
      version: 1,
      anchor: anchor().unwrap(),
    };
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["anchor"]["source"], 1);
    assert_eq!(
      serde_json::to_string(&record).unwrap(),
      r#"{"version":1,"anchor":{"hardware_id":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","source":1}}"#
    );

    for source in [0, 4] {
      let json = serde_json::json!({
        "version": 1,
        "anchor": {
          "hardware_id": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
          "source": source
        }
      });
      assert!(serde_json::from_value::<Record>(json).is_err());
    }
  }
}
