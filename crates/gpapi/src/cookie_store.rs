use std::{
  fs,
  io::Write,
  path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::credential::AuthCookieCredential;

#[cfg(unix)]
#[path = "cookie_store/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "cookie_store/windows.rs"]
mod platform;

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

pub fn cookie_path(custom: Option<&str>) -> Option<PathBuf> {
  match custom {
    Some(path) => Some(PathBuf::from(path)),
    None => platform::default_path(),
  }
}

pub fn load(path: &Path, server: &str, host_id: &str) -> Option<StoredCookie> {
  let bytes = zeroize::Zeroizing::new(platform::decode(fs::read(path).ok()?).ok()?);
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
  platform::prepare_directory(parent)?;

  let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
  tmp.write_all(&platform::encode(serde_json::to_vec(stored)?)?)?;
  tmp.flush()?;
  platform::prepare_file(tmp.path())?;
  tmp.persist(path)?;
  Ok(())
}

pub fn clear(path: &Path) {
  let _ = fs::remove_file(path);
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

  #[cfg(windows)]
  #[test]
  fn windows_cookie_cache_uses_profile_storage_and_protects_credentials() {
    let default_path = cookie_path(None).unwrap();
    assert!(default_path.starts_with(PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cookie.json");
    save(&path, &sample()).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(
      !bytes
        .windows(b"prelogon-auth".len())
        .any(|part| part == b"prelogon-auth")
    );
    assert!(load(&path, "vpn.example.com", "host-1").is_some());
  }

  #[test]
  fn roundtrip_and_mode_0600() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cookie.json");
    let s = sample();
    save(&path, &s).unwrap();

    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
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
