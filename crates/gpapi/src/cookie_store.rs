use std::{
  fs,
  io::Write,
  os::unix::fs::PermissionsExt,
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
  pub last_gateway: String,
  pub auth_cookie: AuthCookieCredential,
  pub saved_at: u64,
}

impl StoredCookie {
  pub fn new(server: String, username: String, last_gateway: String, auth_cookie: AuthCookieCredential) -> Self {
    let saved_at = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_secs())
      .unwrap_or(0);
    Self {
      version: STORE_VERSION,
      server,
      username,
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

pub fn load(path: &Path, server: &str) -> Option<StoredCookie> {
  let bytes = fs::read(path).ok()?;
  let stored: StoredCookie = serde_json::from_slice(&bytes).ok()?;
  if stored.version != STORE_VERSION || stored.server != server {
    return None;
  }
  Some(stored)
}

pub fn save(path: &Path, stored: &StoredCookie) -> anyhow::Result<()> {
  let parent = path
    .parent()
    .ok_or_else(|| anyhow::anyhow!("cookie path has no parent directory"))?;
  fs::create_dir_all(parent)?;
  fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;

  let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
  tmp.write_all(&serde_json::to_vec(stored)?)?;
  tmp.flush()?;
  fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o600))?;
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

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "cookie file must be mode 0600, got {:o}", mode);

    let loaded = load(&path, "vpn.example.com").unwrap();
    assert_eq!(loaded.username, "alice");
    assert_eq!(loaded.last_gateway, "gw1.example.com");
    assert_eq!(loaded.auth_cookie.user_auth_cookie(), "user-auth");

    assert!(load(&path, "other.example.com").is_none(), "must reject mismatched server");
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
    assert!(load(&path, "vpn.example.com").is_none());
  }
}
