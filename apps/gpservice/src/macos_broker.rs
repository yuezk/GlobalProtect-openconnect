use std::{
  os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
  path::{Path, PathBuf},
  sync::Arc,
};

use anyhow::{Context, bail};
use log::{info, warn};
use nix::unistd::Uid;
use tokio::net::{UnixListener, UnixStream};
use tokio_util::sync::CancellationToken;

use crate::{credential_lease, session_registry::SessionRegistry};

pub(crate) const CREDENTIAL_SOCKET: &str = "/var/run/com.yuezk.gpgui/credential.sock";

pub(crate) struct CredentialServer {
  registry: Arc<SessionRegistry>,
}

impl CredentialServer {
  pub(crate) fn new(registry: Arc<SessionRegistry>) -> Self {
    Self { registry }
  }

  pub(crate) async fn start(self, cancel: CancellationToken) -> anyhow::Result<()> {
    if !Uid::effective().is_root() {
      bail!("macOS brokered mode requires root privileges");
    }

    let path = prepare_socket(Path::new(CREDENTIAL_SOCKET), 0)?;
    let listener = UnixListener::bind(&path)
      .with_context(|| format!("Failed to bind broker credential socket at {}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    info!("Broker credential socket listening at {}", path.display());

    loop {
      tokio::select! {
        _ = cancel.cancelled() => break,
        accepted = listener.accept() => {
          let (stream, _) = accepted?;
          if peer_uid(&stream)? != 0 {
            warn!("Rejected non-root broker credential client");
            continue;
          }
          let registry = Arc::clone(&self.registry);
          tokio::spawn(async move {
            if let Err(err) = credential_lease::serve(stream, registry).await {
              warn!("Broker credential lease ended: {err}");
            }
          });
        }
      }
    }

    drop(listener);
    remove_socket(&path, 0)?;
    Ok(())
  }
}

fn prepare_socket(path: &Path, owner_uid: u32) -> anyhow::Result<PathBuf> {
  let parent = path
    .parent()
    .context("Broker credential socket has no parent directory")?;
  if !parent.exists() {
    std::fs::create_dir_all(parent)?;
  }

  let metadata = std::fs::symlink_metadata(parent)?;
  if metadata.file_type().is_symlink() || !metadata.is_dir() || metadata.uid() != owner_uid {
    bail!("Broker credential directory must be a real root-owned directory");
  }
  if metadata.permissions().mode() & 0o077 != 0 {
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
  }

  remove_socket(path, owner_uid)?;
  Ok(path.to_path_buf())
}

fn remove_socket(path: &Path, owner_uid: u32) -> anyhow::Result<()> {
  match std::fs::symlink_metadata(path) {
    Ok(metadata) if metadata.file_type().is_socket() && metadata.uid() == owner_uid => {
      std::fs::remove_file(path)?;
    }
    Ok(_) => bail!("Refusing to replace an unsafe broker credential path"),
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
    Err(err) => return Err(err.into()),
  }
  Ok(())
}

fn peer_uid(stream: &UnixStream) -> anyhow::Result<u32> {
  let (uid, _) = nix::unistd::getpeereid(stream)?;
  Ok(uid.as_raw())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn prepares_private_owner_directory_and_removes_owned_socket_only() {
    let temp = tempfile::tempdir().unwrap();
    let owner = Uid::current().as_raw();
    let parent = temp.path().join("runtime");
    let socket = parent.join("credential.sock");

    assert_eq!(prepare_socket(&socket, owner).unwrap(), socket);
    assert_eq!(std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777, 0o700);

    std::fs::write(&socket, "not a socket").unwrap();
    assert!(prepare_socket(&socket, owner).is_err());
  }

  #[tokio::test]
  async fn peer_uid_matches_current_process() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    assert_eq!(peer_uid(&stream).unwrap(), Uid::current().as_raw());
  }
}
