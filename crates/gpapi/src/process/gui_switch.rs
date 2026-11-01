//! Linux GUI switching through the user-owned desktop supervisor.
use anyhow::Context;
use std::os::unix::fs::PermissionsExt;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::{UnixListener, UnixStream},
};

use crate::gui::{GuiSwitchOutcome, GuiSwitchRequest};

pub(crate) const SOCKET_ENV: &str = "GP_DESKTOP_CONTROL_SOCKET";
pub(crate) const READY_ENV: &str = "GP_DESKTOP_READY_SOCKET";

/// Called after the replacement has initialized its native host.
pub fn notify_ready() -> anyhow::Result<()> {
  use std::io::Write;
  let Some(path) = std::env::var_os(READY_ENV) else {
    return Ok(());
  };
  let mut socket = std::os::unix::net::UnixStream::connect(path).context("Could not notify the desktop supervisor")?;
  socket.write_all(&[1])?;
  Ok(())
}

pub async fn request(selection: GuiSwitchRequest) -> anyhow::Result<GuiSwitchOutcome> {
  let path = std::env::var_os(SOCKET_ENV).context("GUI switching requires a desktop supervisor")?;
  request_at(std::path::Path::new(&path), selection).await
}

pub(super) async fn request_at(
  path: &std::path::Path,
  selection: GuiSwitchRequest,
) -> anyhow::Result<GuiSwitchOutcome> {
  let mut socket = UnixStream::connect(path)
    .await
    .context("Could not contact the desktop supervisor")?;
  socket.write_all(&serde_json::to_vec(&selection)?).await?;
  socket.shutdown().await?;
  let mut reply = Vec::new();
  socket.take(4096).read_to_end(&mut reply).await?;
  let result: Result<GuiSwitchOutcome, String> =
    serde_json::from_slice(&reply).context("Invalid desktop supervisor response")?;
  result.map_err(anyhow::Error::msg)
}

pub(crate) struct SwitchListener {
  pub listener: UnixListener,
  directory: tempfile::TempDir,
}

impl SwitchListener {
  pub fn new() -> anyhow::Result<Self> {
    let directory = tempfile::Builder::new().prefix("gp-desktop-").tempdir()?;
    let path = directory.path().join("control");
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(Self { listener, directory })
  }

  pub fn path(&self) -> std::path::PathBuf {
    self.directory.path().join("control")
  }

  pub async fn ready(&self) -> anyhow::Result<()> {
    let (mut socket, _) = self.listener.accept().await?;
    anyhow::ensure!(
      socket.peer_cred()?.uid() == uzers::get_effective_uid(),
      "Desktop supervisor user mismatch"
    );
    let mut byte = [0];
    socket.read_exact(&mut byte).await?;
    anyhow::ensure!(byte == [1], "Invalid GUI startup notification");
    Ok(())
  }
}

pub(crate) async fn receive(socket: &mut UnixStream) -> anyhow::Result<GuiSwitchRequest> {
  anyhow::ensure!(
    socket.peer_cred()?.uid() == uzers::get_effective_uid(),
    "Desktop supervisor user mismatch"
  );
  let mut request = Vec::new();
  tokio::time::timeout(
    std::time::Duration::from_secs(5),
    socket.take(128).read_to_end(&mut request),
  )
  .await??;
  serde_json::from_slice(&request).context("Invalid GUI selection")
}

pub(crate) async fn reply(socket: &mut UnixStream, result: Result<GuiSwitchOutcome, String>) -> anyhow::Result<()> {
  socket.write_all(&serde_json::to_vec(&result)?).await?;
  socket.shutdown().await?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn private_switch_socket_preserves_selection_and_reports_failure() {
    let selection = GuiSwitchRequest {
      variant: crate::gui::GuiVariant::Nsh,
      allow_download: false,
    };
    let control = SwitchListener::new().unwrap();
    let path = control.path();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    let client = tokio::spawn(async move {
      let mut socket = UnixStream::connect(path).await.unwrap();
      socket
        .write_all(&serde_json::to_vec(&selection).unwrap())
        .await
        .unwrap();
      socket.shutdown().await.unwrap();
      let mut reply = Vec::new();
      socket.read_to_end(&mut reply).await.unwrap();
      serde_json::from_slice::<Result<GuiSwitchOutcome, String>>(&reply).unwrap()
    });
    let (mut socket, _) = control.listener.accept().await.unwrap();
    assert_eq!(receive(&mut socket).await.unwrap(), selection);
    reply(&mut socket, Err("Selected GUI could not start".into()))
      .await
      .unwrap();
    assert_eq!(client.await.unwrap().unwrap_err(), "Selected GUI could not start");
  }
}
