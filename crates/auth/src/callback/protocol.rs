//! Bounded authentication callback wire format, shared by local transports.
use anyhow::bail;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub(super) const CALLBACK_SCHEME: &str = "globalprotectcallback";
pub(super) const PROTOCOL_MAGIC: &[u8; 4] = b"GPAC";
pub(super) const PROTOCOL_VERSION: u8 = 1;
pub(super) const MAX_CALLBACK_SIZE: usize = 1024 * 1024;
pub(super) const IO_TIMEOUT: Duration = Duration::from_secs(5);

// Acceptance acknowledges delivery, including an authentication failure.
pub(super) const STATUS_ACCEPTED: u8 = 0;
pub(super) const STATUS_INVALID_CALLBACK: u8 = 1;
pub(super) const STATUS_UNSUPPORTED_VERSION: u8 = 2;

pub fn validate_auth_callback(callback: &str) -> anyhow::Result<()> {
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

pub(super) enum CallbackReadError {
  UnsupportedVersion,
  Invalid,
  Io,
}

pub(super) async fn read_callback(
  stream: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
) -> Result<String, CallbackReadError> {
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
  validate_auth_callback(&callback).map_err(|_| CallbackReadError::Invalid)?;
  Ok(callback)
}

pub(super) async fn write_response(
  stream: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
  status: u8,
) -> anyhow::Result<()> {
  stream.write_u8(PROTOCOL_VERSION).await?;
  stream.write_u8(status).await?;
  Ok(())
}
