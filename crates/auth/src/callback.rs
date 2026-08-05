use std::{
  fs::{self, DirBuilder, OpenOptions, TryLockError},
  io::ErrorKind,
  os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
  path::{Path, PathBuf},
  time::Duration,
};

use anyhow::{Context, bail};
use gpapi::auth::SamlAuthData;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::{UnixListener, UnixStream},
  time::timeout,
};

const CALLBACK_SCHEME: &str = "globalprotectcallback";
const PROTOCOL_MAGIC: &[u8; 4] = b"GPAC";
const PROTOCOL_VERSION: u8 = 1;
const MAX_CALLBACK_SIZE: usize = 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

const STATUS_ACCEPTED: u8 = 0;
const STATUS_INVALID_CALLBACK: u8 = 1;
const STATUS_UNSUPPORTED_VERSION: u8 = 2;

pub struct AuthCallbackReceiver {
  listener: UnixListener,
  socket_path: PathBuf,
  _lock_file: fs::File,
}

impl AuthCallbackReceiver {
  pub fn bind() -> anyhow::Result<Self> {
    let socket_path = callback_socket_path();
    prepare_socket_directory(&socket_path)?;
    Self::bind_at(socket_path)
  }

  fn bind_at(socket_path: PathBuf) -> anyhow::Result<Self> {
    let lock_file = acquire_lock(&socket_path)?;
    match fs::symlink_metadata(&socket_path) {
      Ok(_) => remove_stale_socket(&socket_path)?,
      Err(err) if err.kind() == ErrorKind::NotFound => {}
      Err(err) => return Err(err.into()),
    }
    finish_bind(UnixListener::bind(&socket_path)?, socket_path, lock_file)
  }

  pub async fn receive(&self) -> anyhow::Result<SamlAuthData> {
    loop {
      let (mut stream, _) = self.listener.accept().await?;
      let callback = match timeout(IO_TIMEOUT, read_callback(&mut stream)).await {
        Ok(callback) => callback,
        Err(_) => continue,
      };
      match callback {
        Ok(callback) => match SamlAuthData::from_gpcallback(&callback) {
          Ok(auth_data) => {
            let _ = write_response(&mut stream, STATUS_ACCEPTED).await;
            return Ok(auth_data);
          }
          Err(_) => {
            let _ = write_response(&mut stream, STATUS_INVALID_CALLBACK).await;
          }
        },
        Err(CallbackReadError::UnsupportedVersion) => {
          let _ = write_response(&mut stream, STATUS_UNSUPPORTED_VERSION).await;
        }
        Err(CallbackReadError::Invalid) => {
          let _ = write_response(&mut stream, STATUS_INVALID_CALLBACK).await;
        }
        Err(CallbackReadError::Io) => continue,
      }
    }
  }
}

impl Drop for AuthCallbackReceiver {
  fn drop(&mut self) {
    if fs::symlink_metadata(&self.socket_path)
      .is_ok_and(|metadata| metadata.file_type().is_socket() && metadata.uid() == uzers::get_current_uid())
    {
      let _ = fs::remove_file(&self.socket_path);
    }
  }
}

pub async fn forward_auth_callback(callback: &str) -> anyhow::Result<()> {
  validate_callback(callback)?;
  forward_auth_callback_to(callback, &callback_socket_path()).await
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub fn ensure_auth_callback_handler() -> anyhow::Result<()> {
  const MIME_TYPE: &str = "x-scheme-handler/globalprotectcallback";
  const HANDLER: &str = "gpauth.desktop";

  let current_handler = query_callback_handler(MIME_TYPE)?;
  if !should_register_handler(&current_handler)? {
    return Ok(());
  }

  let status = std::process::Command::new("xdg-mime")
    .args(["default", HANDLER, MIME_TYPE])
    .status()
    .context("xdg-mime is required for external-browser authentication")?;
  if !status.success() {
    bail!("Failed to register the globalprotectcallback URL handler");
  }
  if query_callback_handler(MIME_TYPE)? != HANDLER {
    bail!("The globalprotectcallback URL handler registration did not take effect");
  }

  Ok(())
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
fn query_callback_handler(mime_type: &str) -> anyhow::Result<String> {
  let output = std::process::Command::new("xdg-mime")
    .args(["query", "default", mime_type])
    .output()
    .context("xdg-mime is required for external-browser authentication")?;
  if !output.status.success() {
    bail!("Failed to query the globalprotectcallback URL handler");
  }
  Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
pub fn ensure_auth_callback_handler() -> anyhow::Result<()> {
  Ok(())
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", test))]
fn should_register_handler(current_handler: &str) -> anyhow::Result<bool> {
  match current_handler {
    "gpauth.desktop" => Ok(false),
    "" | "gpgui.desktop" => Ok(true),
    handler => bail!("The globalprotectcallback URL scheme is owned by {handler}"),
  }
}

async fn forward_auth_callback_to(callback: &str, socket_path: &Path) -> anyhow::Result<()> {
  validate_callback(callback)?;

  let (version, status) = timeout(IO_TIMEOUT, async {
    let mut stream = UnixStream::connect(socket_path)
      .await
      .context("No external-browser authentication is waiting for a callback")?;
    stream.write_all(PROTOCOL_MAGIC).await?;
    stream.write_u8(PROTOCOL_VERSION).await?;
    stream.write_u32(callback.len() as u32).await?;
    stream.write_all(callback.as_bytes()).await?;
    Ok::<_, anyhow::Error>((stream.read_u8().await?, stream.read_u8().await?))
  })
  .await
  .context("Timed out forwarding the authentication callback")??;
  if version != PROTOCOL_VERSION {
    bail!("Authentication callback protocol version mismatch");
  }

  match status {
    STATUS_ACCEPTED => Ok(()),
    STATUS_INVALID_CALLBACK => bail!("Authentication callback was rejected"),
    STATUS_UNSUPPORTED_VERSION => bail!("Authentication callback protocol version is unsupported"),
    _ => bail!("Authentication callback handler returned an unknown status"),
  }
}

fn callback_socket_path() -> PathBuf {
  let uid = uzers::get_current_uid();

  #[cfg(target_os = "linux")]
  {
    let runtime_dir = PathBuf::from(format!("/run/user/{uid}"));
    if fs::symlink_metadata(&runtime_dir).is_ok_and(|metadata| {
      metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && metadata.uid() == uid
        && metadata.permissions().mode() & 0o077 == 0
    }) {
      return runtime_dir.join("gp-connect/auth-callback.sock");
    }
  }

  PathBuf::from(format!("/tmp/gp-connect-{uid}/auth-callback.sock"))
}

fn prepare_socket_directory(socket_path: &Path) -> anyhow::Result<()> {
  let directory = socket_path
    .parent()
    .ok_or_else(|| anyhow::anyhow!("Authentication callback socket has no parent directory"))?;

  match DirBuilder::new().mode(0o700).create(directory) {
    Ok(()) => {}
    Err(err) if err.kind() == ErrorKind::AlreadyExists => {
      let metadata = fs::symlink_metadata(directory)?;
      if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("Authentication callback runtime path is not a directory");
      }
      if metadata.uid() != uzers::get_current_uid() {
        bail!("Authentication callback runtime directory has the wrong owner");
      }
      if metadata.permissions().mode() & 0o077 != 0 {
        bail!("Authentication callback runtime directory permissions are too broad");
      }
    }
    Err(err) => return Err(err.into()),
  }

  Ok(())
}

fn finish_bind(
  listener: UnixListener,
  socket_path: PathBuf,
  lock_file: fs::File,
) -> anyhow::Result<AuthCallbackReceiver> {
  fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
  Ok(AuthCallbackReceiver {
    listener,
    socket_path,
    _lock_file: lock_file,
  })
}

fn acquire_lock(socket_path: &Path) -> anyhow::Result<fs::File> {
  let lock_path = socket_path.with_file_name("auth-callback.lock");
  let lock_file = OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .truncate(false)
    .mode(0o600)
    .open(lock_path)?;
  lock_file.set_permissions(fs::Permissions::from_mode(0o600))?;

  match lock_file.try_lock() {
    Ok(()) => Ok(lock_file),
    Err(TryLockError::WouldBlock) => bail!("Another external-browser authentication is already in progress"),
    Err(TryLockError::Error(err)) => Err(err.into()),
  }
}

fn remove_stale_socket(socket_path: &Path) -> anyhow::Result<()> {
  let metadata = fs::symlink_metadata(socket_path)?;
  if !metadata.file_type().is_socket() || metadata.uid() != uzers::get_current_uid() {
    bail!("Authentication callback socket is not owned by the current user");
  }

  fs::remove_file(socket_path)?;
  Ok(())
}

fn validate_callback(callback: &str) -> anyhow::Result<()> {
  if callback.len() > MAX_CALLBACK_SIZE {
    bail!("Authentication callback exceeds the maximum supported size");
  }

  let Some((scheme, data)) = callback.split_once(':') else {
    bail!("Authentication callback URL is invalid");
  };
  if scheme != CALLBACK_SCHEME || data.is_empty() {
    bail!("Authentication callback URL is invalid");
  }

  Ok(())
}

enum CallbackReadError {
  UnsupportedVersion,
  Invalid,
  Io,
}

async fn read_callback(stream: &mut UnixStream) -> Result<String, CallbackReadError> {
  let mut magic = [0_u8; 4];
  stream.read_exact(&mut magic).await.map_err(|_| CallbackReadError::Io)?;
  if &magic != PROTOCOL_MAGIC {
    return Err(CallbackReadError::Invalid);
  }

  let version = stream.read_u8().await.map_err(|_| CallbackReadError::Io)?;
  if version != PROTOCOL_VERSION {
    return Err(CallbackReadError::UnsupportedVersion);
  }

  let length = stream.read_u32().await.map_err(|_| CallbackReadError::Io)? as usize;
  if length == 0 || length > MAX_CALLBACK_SIZE {
    return Err(CallbackReadError::Invalid);
  }

  let mut payload = vec![0; length];
  stream
    .read_exact(&mut payload)
    .await
    .map_err(|_| CallbackReadError::Io)?;
  let callback = String::from_utf8(payload).map_err(|_| CallbackReadError::Invalid)?;
  validate_callback(&callback).map_err(|_| CallbackReadError::Invalid)?;
  Ok(callback)
}

async fn write_response(stream: &mut UnixStream, status: u8) -> anyhow::Result<()> {
  stream.write_u8(PROTOCOL_VERSION).await?;
  stream.write_u8(status).await?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use tempfile::tempdir;

  use super::*;

  const VALID_CALLBACK: &str = "globalprotectcallback:cas-as=1&un=alice@example.com&token=very_long_string";

  #[test]
  fn validates_supported_callback_scheme() {
    assert!(validate_callback(VALID_CALLBACK).is_ok());
    assert!(validate_callback("https://example.com").is_err());
    assert!(validate_callback("globalprotectcallback:").is_err());
    assert!(validate_callback(&format!("globalprotectcallback:{}", "x".repeat(MAX_CALLBACK_SIZE))).is_err());
  }

  #[test]
  fn preserves_unrelated_callback_handler() {
    assert!(!should_register_handler("gpauth.desktop").unwrap());
    assert!(should_register_handler("").unwrap());
    assert!(should_register_handler("gpgui.desktop").unwrap());
    assert!(should_register_handler("other-vpn.desktop").is_err());
  }

  #[tokio::test]
  async fn forwards_callback_to_waiting_receiver() {
    let directory = tempdir().unwrap();
    let socket_path = directory.path().join("callback.sock");
    let receiver = AuthCallbackReceiver::bind_at(socket_path.clone()).unwrap();
    assert_eq!(
      fs::symlink_metadata(&socket_path).unwrap().permissions().mode() & 0o777,
      0o600
    );

    let sender = tokio::spawn(async move { forward_auth_callback_to(VALID_CALLBACK, &socket_path).await });
    let auth_data = receiver.receive().await.unwrap();

    sender.await.unwrap().unwrap();
    assert_eq!(auth_data.username(), "alice@example.com");
    assert_eq!(auth_data.token(), Some("very_long_string"));
    let socket_path = receiver.socket_path.clone();
    drop(receiver);
    assert!(!socket_path.exists());
  }

  #[tokio::test]
  async fn rejects_bad_requests_and_keeps_waiting() {
    let directory = tempdir().unwrap();
    let socket_path = directory.path().join("callback.sock");
    let receiver = AuthCallbackReceiver::bind_at(socket_path.clone()).unwrap();

    let sender = tokio::spawn(async move {
      assert_eq!(
        send_raw_callback(&socket_path, PROTOCOL_VERSION, "https://example.com").await,
        STATUS_INVALID_CALLBACK
      );
      assert_eq!(
        send_raw_callback(&socket_path, PROTOCOL_VERSION + 1, VALID_CALLBACK).await,
        STATUS_UNSUPPORTED_VERSION
      );
      forward_auth_callback_to(VALID_CALLBACK, &socket_path).await
    });
    let auth_data = receiver.receive().await.unwrap();

    sender.await.unwrap().unwrap();
    assert_eq!(auth_data.username(), "alice@example.com");
  }

  #[tokio::test]
  async fn ignores_sender_that_disconnects_before_sending() {
    let directory = tempdir().unwrap();
    let socket_path = directory.path().join("callback.sock");
    let receiver = AuthCallbackReceiver::bind_at(socket_path.clone()).unwrap();

    drop(UnixStream::connect(&socket_path).await.unwrap());
    let sender = tokio::spawn(async move { forward_auth_callback_to(VALID_CALLBACK, &socket_path).await });
    let auth_data = receiver.receive().await.unwrap();

    sender.await.unwrap().unwrap();
    assert_eq!(auth_data.username(), "alice@example.com");
  }

  #[tokio::test]
  async fn replaces_owned_stale_socket() {
    let directory = tempdir().unwrap();
    let socket_path = directory.path().join("callback.sock");
    drop(std::os::unix::net::UnixListener::bind(&socket_path).unwrap());

    let receiver = AuthCallbackReceiver::bind_at(socket_path.clone()).unwrap();
    assert!(socket_path.exists());
    drop(receiver);
    assert!(!socket_path.exists());
  }

  #[tokio::test]
  async fn reports_when_no_receiver_is_active() {
    let directory = tempdir().unwrap();
    let socket_path = directory.path().join("missing.sock");

    let err = forward_auth_callback_to(VALID_CALLBACK, &socket_path)
      .await
      .unwrap_err();
    assert!(err.to_string().contains("No external-browser authentication"));
  }

  #[test]
  fn rejects_runtime_directory_with_broad_permissions() {
    let directory = tempdir().unwrap();
    let runtime_dir = directory.path().join("runtime");
    fs::create_dir(&runtime_dir).unwrap();
    fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o755)).unwrap();

    let err = prepare_socket_directory(&runtime_dir.join("callback.sock")).unwrap_err();
    assert!(err.to_string().contains("permissions are too broad"));
  }

  #[tokio::test]
  async fn rejects_second_active_receiver() {
    let directory = tempdir().unwrap();
    let socket_path = directory.path().join("callback.sock");
    let _receiver = AuthCallbackReceiver::bind_at(socket_path.clone()).unwrap();

    let err = AuthCallbackReceiver::bind_at(socket_path).err().unwrap();
    assert!(err.to_string().contains("already in progress"));
  }

  async fn send_raw_callback(socket_path: &Path, version: u8, callback: &str) -> u8 {
    let mut stream = UnixStream::connect(socket_path).await.unwrap();
    stream.write_all(PROTOCOL_MAGIC).await.unwrap();
    stream.write_u8(version).await.unwrap();
    stream.write_u32(callback.len() as u32).await.unwrap();
    stream.write_all(callback.as_bytes()).await.unwrap();
    assert_eq!(stream.read_u8().await.unwrap(), PROTOCOL_VERSION);
    stream.read_u8().await.unwrap()
  }
}
