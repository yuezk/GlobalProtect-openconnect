//! Per-user Windows named pipe for browser authentication callbacks.
use anyhow::{Context, bail};
use gpapi::{auth::SamlAuthData, error::AuthDataParseError};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::windows::named_pipe::{ClientOptions, NamedPipeServer},
  time::timeout,
};
#[path = "protocol.rs"]
mod protocol;
pub use protocol::validate_auth_callback;
use protocol::*;

pub struct AuthCallbackReceiver {
  listener: NamedPipeServer,
}
impl AuthCallbackReceiver {
  pub fn bind() -> anyhow::Result<Self> {
    Ok(Self {
      listener: gpapi::windows::create_user_pipe(&pipe_name()?, true)
        .context("Another browser authentication is already waiting")?,
    })
  }
  pub async fn receive(mut self) -> anyhow::Result<SamlAuthData> {
    loop {
      self.listener.connect().await?;
      let mut stream = &mut self.listener;
      let callback = timeout(IO_TIMEOUT, read_callback(&mut stream)).await;
      let result = match callback {
        Ok(Ok(callback)) => match SamlAuthData::from_gpcallback(&callback) {
          Ok(data) => {
            let _ = write_response(&mut stream, STATUS_ACCEPTED).await;
            Some(Ok(data))
          }
          Err(error @ AuthDataParseError::AuthenticationFailed(_)) => {
            let _ = write_response(&mut stream, STATUS_ACCEPTED).await;
            Some(Err(error.into()))
          }
          Err(_) => {
            let _ = write_response(&mut stream, STATUS_INVALID_CALLBACK).await;
            None
          }
        },
        Ok(Err(CallbackReadError::UnsupportedVersion)) => {
          let _ = write_response(&mut stream, STATUS_UNSUPPORTED_VERSION).await;
          None
        }
        _ => None,
      };
      // DisconnectNamedPipe discards unread response bytes. Wait for the client
      // to confirm it consumed the response before disconnecting this instance.
      let _ = timeout(IO_TIMEOUT, stream.read_u8()).await;
      self.listener.disconnect()?;
      if let Some(result) = result {
        return result;
      }
    }
  }
}
fn pipe_name() -> anyhow::Result<String> {
  Ok(format!(
    r"\\.\pipe\GPConnect.Auth.{}",
    gpapi::windows::current_user_sid()?
  ))
}

pub async fn forward_auth_callback(callback: &str) -> anyhow::Result<()> {
  validate_auth_callback(callback)?;
  timeout(IO_TIMEOUT, async {
    let mut stream = ClientOptions::new()
      .open(pipe_name()?)
      .context("No external-browser authentication is waiting for a callback")?;
    stream.write_all(PROTOCOL_MAGIC).await?;
    stream.write_u8(PROTOCOL_VERSION).await?;
    stream.write_u32(callback.len() as u32).await?;
    stream.write_all(callback.as_bytes()).await?;
    let version = stream.read_u8().await?;
    let status = stream.read_u8().await?;
    stream.write_u8(status).await?;
    anyhow::ensure!(
      version == PROTOCOL_VERSION,
      "Authentication callback protocol version mismatch"
    );
    match status {
      STATUS_ACCEPTED => Ok(()),
      STATUS_INVALID_CALLBACK => bail!("Authentication callback was rejected"),
      _ => bail!("Authentication callback protocol is unsupported"),
    }
  })
  .await
  .context("Timed out forwarding the authentication callback")?
}

pub fn ensure_auth_callback_handler() -> anyhow::Result<()> {
  use winreg::{RegKey, enums::HKEY_CURRENT_USER};
  let executable = common::binary_paths::gpauth();
  anyhow::ensure!(executable.is_file(), "The authentication helper is missing");
  let root = RegKey::predef(HKEY_CURRENT_USER);
  let (scheme, _) = root.create_subkey(r"Software\Classes\globalprotectcallback")?;
  scheme.set_value("", &"URL:GlobalProtect authentication callback")?;
  scheme.set_value("URL Protocol", &"")?;
  let (command, _) = scheme.create_subkey(r"shell\open\command")?;
  // URL association activation requires a shell path, not Rust's verbatim
  // canonical path (\\?\C:\...), even though CreateProcess accepts both.
  let executable = dunce::simplified(&executable);
  command.set_value("", &format!("\"{}\" auth-callback \"%1\"", executable.display()))?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  #[tokio::test]
  async fn forwards_browser_callback_over_private_windows_pipe() {
    let receiver = AuthCallbackReceiver::bind().unwrap();
    // The first-instance flag preserves one pending browser authentication per user.
    assert!(AuthCallbackReceiver::bind().is_err());
    let sender = tokio::spawn(async {
      forward_auth_callback("globalprotectcallback:cas-as=1&un=alice@example.com&token=test-token").await
    });
    let data = timeout(std::time::Duration::from_secs(5), receiver.receive())
      .await
      .unwrap()
      .unwrap();
    sender.await.unwrap().unwrap();
    assert_eq!(data.username(), "alice@example.com");
    assert_eq!(data.token(), Some("test-token"));
  }
}
