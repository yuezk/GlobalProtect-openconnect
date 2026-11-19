use std::{
  fs, io,
  os::unix::fs::PermissionsExt,
  path::{Path, PathBuf},
};

pub(super) fn default_path() -> Option<PathBuf> {
  let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
  Some(PathBuf::from(home).join(".config/gpclient/cookie.json"))
}

pub(super) fn encode(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
  Ok(bytes)
}

pub(super) use encode as decode;

pub(super) fn prepare_directory(path: &Path) -> io::Result<()> {
  fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

pub(super) fn prepare_file(path: &Path) -> io::Result<()> {
  fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}
