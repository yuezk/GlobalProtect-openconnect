use std::path::{Path, PathBuf};

use common::constants::GP_SERVICE_LOCK_FILE;
use thiserror::Error;
use tokio::fs;

pub struct LockFile {
  path: PathBuf,
  pid: u32,
}

pub fn dev_service_lock_file_path(bootstrap_socket: impl AsRef<Path>) -> PathBuf {
  bootstrap_socket.as_ref().with_file_name("gpservice.lock")
}

impl LockFile {
  pub fn new<P: Into<PathBuf>>(path: P, pid: u32) -> Self {
    Self { path: path.into(), pid }
  }

  pub fn exists(&self) -> bool {
    self.path.exists()
  }

  pub fn lock(&self, content: &str) -> anyhow::Result<()> {
    let content = format!("{}:{}", self.pid, content);
    std::fs::write(&self.path, content)?;
    Ok(())
  }

  pub fn unlock(&self) -> anyhow::Result<()> {
    match std::fs::read_to_string(&self.path) {
      Ok(content) if LockInfo::parse(&content).is_ok_and(|info| info.pid == self.pid) => {
        std::fs::remove_file(&self.path)?;
        Ok(())
      }
      Ok(_) => Ok(()),
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
      Err(err) => Err(err.into()),
    }
  }

  pub async fn check_health(&self) -> bool {
    match std::fs::read_to_string(&self.path) {
      Ok(content) => {
        let url = format!("http://127.0.0.1:{}/health", content.trim());

        match reqwest::get(&url).await {
          Ok(resp) => resp.status().is_success(),
          Err(_) => false,
        }
      }
      Err(_) => false,
    }
  }
}

#[derive(Error, Debug)]
pub enum LockFileError {
  #[error("Failed to read lock file: {0}")]
  IoError(#[from] std::io::Error),

  #[error("Invalid lock file format: expected 'pid:port'")]
  InvalidFormat,

  #[error("Invalid PID value: {0}")]
  InvalidPid(std::num::ParseIntError),

  #[error("Invalid port value: {0}")]
  InvalidPort(std::num::ParseIntError),
}

pub struct LockInfo {
  pub pid: u32,
  pub port: u32,
}

impl LockInfo {
  async fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, LockFileError> {
    let content = fs::read_to_string(path).await?;
    Self::parse(&content)
  }

  fn parse(content: &str) -> Result<Self, LockFileError> {
    let mut parts = content.trim().split(':');

    let pid = parts
      .next()
      .ok_or(LockFileError::InvalidFormat)?
      .parse()
      .map_err(LockFileError::InvalidPid)?;

    let port = parts
      .next()
      .ok_or(LockFileError::InvalidFormat)?
      .parse()
      .map_err(LockFileError::InvalidPort)?;

    // Ensure there are no extra parts after pid:port
    if parts.next().is_some() {
      return Err(LockFileError::InvalidFormat);
    }

    Ok(Self { pid, port })
  }
}

pub async fn gpservice_lock_info() -> Result<LockInfo, LockFileError> {
  gpservice_lock_info_at(GP_SERVICE_LOCK_FILE).await
}

pub async fn gpservice_lock_info_at(path: impl AsRef<Path>) -> Result<LockInfo, LockFileError> {
  LockInfo::from_file(path).await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_parse_valid_input() {
    let info = LockInfo::parse("1234:8080").unwrap();
    assert_eq!(info.pid, 1234);
    assert_eq!(info.port, 8080);
  }

  #[test]
  fn test_parse_invalid_format() {
    assert!(matches!(
      LockInfo::parse("123:456:789"),
      Err(LockFileError::InvalidFormat)
    ));
  }

  #[test]
  fn test_parse_invalid_numbers() {
    assert!(matches!(LockInfo::parse("abc:8080"), Err(LockFileError::InvalidPid(_))));

    assert!(matches!(
      LockInfo::parse("1234:abc"),
      Err(LockFileError::InvalidPort(_))
    ));
  }

  #[test]
  fn dev_service_lock_file_is_next_to_the_bootstrap_socket() {
    assert_eq!(
      dev_service_lock_file_path("/var/run/gpservice-dev-123/dev-bootstrap.sock"),
      PathBuf::from("/var/run/gpservice-dev-123/gpservice.lock")
    );
  }

  #[test]
  fn unlock_removes_lock_owned_by_process() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("gpservice.lock");
    let lock = LockFile::new(&path, 1234);
    lock.lock("8080").unwrap();

    lock.unlock().unwrap();

    assert!(!path.exists());
  }

  #[test]
  fn unlock_preserves_lock_owned_by_another_process() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("gpservice.lock");
    let owner = LockFile::new(&path, 1234);
    let other = LockFile::new(&path, 5678);
    owner.lock("8080").unwrap();

    other.unlock().unwrap();

    assert_eq!(std::fs::read_to_string(path).unwrap(), "1234:8080");
  }

  #[test]
  fn unlock_ignores_missing_or_malformed_lock() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("gpservice.lock");
    let lock = LockFile::new(&path, 1234);

    lock.unlock().unwrap();
    std::fs::write(&path, "not-a-lock").unwrap();
    lock.unlock().unwrap();

    assert_eq!(std::fs::read_to_string(path).unwrap(), "not-a-lock");
  }
}
