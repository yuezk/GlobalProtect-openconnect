use std::{
  os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
  path::{Path, PathBuf},
  sync::Arc,
};

use anyhow::{Context, bail};
use log::{info, warn};
use nix::unistd::{Uid, chown};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::{UnixListener, UnixStream},
};
use tokio_util::sync::CancellationToken;

use crate::session_registry::SessionRegistry;

pub struct DevBootstrap {
  path: PathBuf,
  allowed_uid: u32,
  registry: Arc<SessionRegistry>,
}

impl DevBootstrap {
  pub fn new(path: PathBuf, allowed_uid: u32, registry: Arc<SessionRegistry>) -> Self {
    Self {
      path,
      allowed_uid,
      registry,
    }
  }

  pub async fn start(self, cancel: CancellationToken) -> anyhow::Result<()> {
    create_default_parent(&self.path, self.allowed_uid)?;
    let path = prepare_socket_path(&self.path, self.allowed_uid)?;
    let listener = UnixListener::bind(&path)
      .with_context(|| format!("Failed to bind debug bootstrap socket at {}", path.display()))?;
    chown(&path, Some(Uid::from_raw(self.allowed_uid)), None)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    info!("Debug credential bootstrap listening at {}", path.display());

    loop {
      tokio::select! {
        _ = cancel.cancelled() => break,
        accepted = listener.accept() => {
          let (stream, _) = accepted?;
          let peer_uid = match peer_uid(&stream) {
            Ok(uid) => uid,
            Err(err) => {
              warn!("Could not verify debug credential client UID: {err}");
              continue;
            }
          };
          if peer_uid != self.allowed_uid {
            warn!("Rejected debug credential client with unexpected UID");
            continue;
          }
          let registry = Arc::clone(&self.registry);
          tokio::spawn(async move {
            if let Err(err) = authorize_gui(stream, registry).await {
              warn!("Debug credential client ended: {err}");
            }
          });
        }
      }
    }

    if path.exists() {
      std::fs::remove_file(&path)?;
    }
    Ok(())
  }
}

fn create_default_parent(path: &Path, allowed_uid: u32) -> anyhow::Result<()> {
  let default = default_socket_path(allowed_uid);
  if path != default {
    return Ok(());
  }
  let parent = path
    .parent()
    .context("Debug bootstrap socket has no parent directory")?;
  std::fs::create_dir_all(parent)?;
  std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o711))?;
  Ok(())
}

fn default_socket_path(allowed_uid: u32) -> PathBuf {
  PathBuf::from(format!("/var/run/gpservice-dev-{allowed_uid}/dev-bootstrap.sock"))
}

fn prepare_socket_path(path: &Path, allowed_uid: u32) -> anyhow::Result<PathBuf> {
  let parent = path
    .parent()
    .context("Debug bootstrap socket has no parent directory")?;
  let parent = std::fs::canonicalize(parent)
    .with_context(|| format!("Debug bootstrap directory does not exist: {}", parent.display()))?;
  let metadata = std::fs::symlink_metadata(&parent)?;
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    bail!("Debug bootstrap parent must be a real directory");
  }
  if metadata.uid() != 0 || metadata.permissions().mode() & 0o777 != 0o711 {
    bail!("Debug bootstrap parent must be owned by root with mode 0711");
  }

  let file_name = path.file_name().context("Debug bootstrap socket has no file name")?;
  let path = parent.join(file_name);

  match std::fs::symlink_metadata(&path) {
    Ok(metadata) => {
      if metadata.file_type().is_symlink()
        || !metadata.file_type().is_socket()
        || metadata.uid() != allowed_uid
        || metadata.permissions().mode() & 0o777 != 0o600
      {
        bail!("Refusing unsafe existing debug bootstrap path");
      }
      std::fs::remove_file(&path)?;
    }
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
    Err(err) => return Err(err.into()),
  }
  Ok(path)
}

async fn authorize_gui(mut stream: UnixStream, registry: Arc<SessionRegistry>) -> anyhow::Result<()> {
  let credential = registry.issue(env!("CARGO_PKG_VERSION"))?;
  let session_id = credential.session_id();
  let result = async {
    let frame = credential.encode_frame()?;
    stream.write_all(&frame).await?;

    let mut buffer = [0_u8; 1];
    loop {
      match stream.read(&mut buffer).await {
        Ok(0) => break,
        Ok(_) => continue,
        Err(err) => return Err(err.into()),
      }
    }
    Ok(())
  }
  .await;
  registry.revoke(session_id);
  result
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> anyhow::Result<u32> {
  use nix::sys::socket::{getsockopt, sockopt};

  let credentials = getsockopt(stream, sockopt::PeerCredentials)?;
  Ok(credentials.uid())
}

#[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
fn peer_uid(stream: &UnixStream) -> anyhow::Result<u32> {
  let (uid, _) = nix::unistd::getpeereid(stream)?;
  Ok(uid.as_raw())
}

#[cfg(test)]
mod tests {
  use std::os::unix::fs::{PermissionsExt, symlink};

  use gpapi::service::transport::{MAX_CREDENTIAL_FRAME, SessionCredential};
  use tokio::io::AsyncReadExt;
  use uuid::Uuid;

  use super::*;

  #[test]
  fn rejects_developer_owned_parent() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = temp.path().join("bootstrap.sock");

    assert!(prepare_socket_path(&socket, Uid::current().as_raw()).is_err());
  }

  #[test]
  fn rejects_symlink_socket_path() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let target = temp.path().join("target");
    std::fs::write(&target, "not a socket").unwrap();
    let socket = temp.path().join("bootstrap.sock");
    symlink(target, &socket).unwrap();

    assert!(prepare_socket_path(&socket, Uid::current().as_raw()).is_err());
  }

  #[test]
  fn custom_parent_is_not_created_by_the_service() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("missing");
    let socket = parent.join("bootstrap.sock");

    create_default_parent(&socket, Uid::current().as_raw()).unwrap();

    assert!(!parent.exists());
  }

  #[test]
  fn default_socket_path_is_stable() {
    assert_eq!(
      default_socket_path(1234),
      PathBuf::from("/var/run/gpservice-dev-1234/dev-bootstrap.sock")
    );
  }

  #[tokio::test]
  async fn anchor_eof_revokes_issued_credential() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let (service, mut client) = UnixStream::pair().unwrap();
    let task = tokio::spawn(authorize_gui(service, Arc::clone(&registry)));

    let mut header = [0_u8; 2];
    client.read_exact(&mut header).await.unwrap();
    let payload_len = u16::from_be_bytes(header) as usize;
    assert!(payload_len + 2 <= MAX_CREDENTIAL_FRAME);
    let mut frame = vec![0_u8; payload_len + 2];
    frame[..2].copy_from_slice(&header);
    client.read_exact(&mut frame[2..]).await.unwrap();
    let credential = SessionCredential::decode_frame(&frame).unwrap();
    drop(client);
    task.await.unwrap().unwrap();

    assert!(matches!(
      registry.begin_handshake(
        credential.service_instance_id(),
        credential.session_id(),
        credential.product_version()
      ),
      Err(crate::session_registry::SessionError::Unauthorized)
    ));
  }

  #[tokio::test]
  async fn peer_uid_matches_current_process() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    assert_eq!(peer_uid(&stream).unwrap(), Uid::current().as_raw());
  }
}
