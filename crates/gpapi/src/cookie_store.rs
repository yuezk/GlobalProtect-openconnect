use std::{
  fs,
  io::Write,
  path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::credential::AuthCookieCredential;

const STORE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StoredCookie {
  pub version: u32,
  pub server: String,
  pub username: String,
  pub host_id: String,
  pub last_gateway: String,
  pub auth_cookie: AuthCookieCredential,
  pub saved_at: u64,
}

impl StoredCookie {
  pub fn new(
    server: String,
    username: String,
    host_id: String,
    last_gateway: String,
    auth_cookie: AuthCookieCredential,
  ) -> Self {
    let saved_at = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_secs())
      .unwrap_or(0);
    Self {
      version: STORE_VERSION,
      server,
      username,
      host_id,
      last_gateway,
      auth_cookie,
      saved_at,
    }
  }
}

pub fn cookie_path(custom: Option<&str>) -> PathBuf {
  if let Some(p) = custom {
    return PathBuf::from(p);
  }
  let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
  PathBuf::from(home).join(".config/gpclient/cookie.json")
}

pub fn load(path: &Path, server: &str, host_id: &str) -> Option<StoredCookie> {
  let bytes = fs::read(path).ok()?;
  let stored: StoredCookie = serde_json::from_slice(&bytes).ok()?;
  if stored.version != STORE_VERSION || stored.server != server || stored.host_id != host_id {
    return None;
  }
  Some(stored)
}

pub fn save(path: &Path, stored: &StoredCookie) -> anyhow::Result<()> {
  let parent = path
    .parent()
    .ok_or_else(|| anyhow::anyhow!("cookie path has no parent directory"))?;
  fs::create_dir_all(parent)?;
  set_private_dir_permissions(parent)?;

  let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
  tmp.write_all(&serde_json::to_vec(stored)?)?;
  tmp.flush()?;
  set_private_file_permissions(tmp.path())?;
  tmp.persist(path)?;
  Ok(())
}

pub fn clear(path: &Path) {
  let _ = fs::remove_file(path);
}

use permissions::{set_private_dir_permissions, set_private_file_permissions};

mod permissions {
  use std::path::Path;

  #[cfg(unix)]
  mod unix {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    pub(super) fn set_private_dir(path: &Path) -> anyhow::Result<()> {
      fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
      Ok(())
    }

    pub(super) fn set_private_file(path: &Path) -> anyhow::Result<()> {
      fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
      Ok(())
    }

    #[cfg(test)]
    pub(super) fn file_mode(path: &Path) -> Option<u32> {
      Some(fs::metadata(path).ok()?.permissions().mode() & 0o777)
    }
  }

  pub(super) fn set_private_dir_permissions(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    unix::set_private_dir(path)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
  }

  pub(super) fn set_private_file_permissions(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    unix::set_private_file(path)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
  }

  #[cfg(test)]
  pub(super) fn file_mode(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    return unix::file_mode(path);

    #[cfg(not(unix))]
    {
      let _ = path;
      None
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn sample() -> StoredCookie {
    StoredCookie::new(
      "vpn.example.com".to_string(),
      "alice".to_string(),
      "host-1".to_string(),
      "gw1.example.com".to_string(),
      AuthCookieCredential::new("alice", "user-auth", "prelogon-auth"),
    )
  }

  #[test]
  fn roundtrip_and_mode_0600() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cookie.json");
    let s = sample();
    save(&path, &s).unwrap();

    if let Some(mode) = permissions::file_mode(&path) {
      assert_eq!(mode, 0o600, "cookie file must be mode 0600, got {:o}", mode);
    }

    let loaded = load(&path, "vpn.example.com", "host-1").unwrap();
    assert_eq!(loaded.username, "alice");
    assert_eq!(loaded.host_id, "host-1");
    assert_eq!(loaded.last_gateway, "gw1.example.com");
    assert_eq!(loaded.auth_cookie.user_auth_cookie(), "user-auth");

    assert!(
      load(&path, "other.example.com", "host-1").is_none(),
      "must reject mismatched server"
    );
    assert!(
      load(&path, "vpn.example.com", "host-2").is_none(),
      "must reject mismatched host id"
    );
    clear(&path);
    assert!(!path.exists());
  }

  #[test]
  fn rejects_version_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cookie.json");
    let mut s = sample();
    s.version = STORE_VERSION + 1;
    save(&path, &s).unwrap();
    assert!(load(&path, "vpn.example.com", "host-1").is_none());
  }
}
