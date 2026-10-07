use std::{
  collections::HashMap,
  fs::File,
  process::{ExitStatus, Stdio},
  sync::Arc,
};

use anyhow::{Context, ensure};
use common::binary_paths;
use tokio::{
  io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
  process::Command,
};
use zeroize::Zeroizing;

use super::{CommandExt, gui_launcher::GuiLauncher};
use crate::service::transport::{MAX_CREDENTIAL_FRAME, SessionCredential};

/// Owns the desktop GUI and its privileged service without leaving the desktop session.
pub struct DesktopLauncher<'a> {
  version: &'a str,
  minimized: bool,
  log_file: Option<String>,
}

impl<'a> DesktopLauncher<'a> {
  pub fn new(version: &'a str) -> Self {
    Self {
      version,
      minimized: false,
      log_file: None,
    }
  }

  pub fn minimized(mut self, minimized: bool) -> Self {
    self.minimized = minimized;
    self
  }

  pub fn log_file(mut self, log_file: &str) -> Self {
    self.log_file = Some(log_file.to_owned());
    self
  }

  pub async fn launch(&self) -> anyhow::Result<ExitStatus> {
    ensure!(
      uzers::get_effective_uid() != 0,
      "The desktop launcher must not run as root"
    );
    let mut command = Command::new_pkexec(binary_paths::gpservice());
    command
      .arg("--desktop-credentials")
      .stdin(Stdio::piped())
      .stdout(Stdio::piped());
    if let Some(path) = &self.log_file {
      command.stderr(Stdio::from(File::create(path)?));
    }
    let mut service = command.spawn().context("Failed to start the privileged service")?;
    let mut input = service.stdin.take().context("Service input pipe is unavailable")?;
    let mut output = service
      .stdout
      .take()
      .context("Service credential pipe is unavailable")?;
    let mut received_credential = false;
    let mut cancelled = false;
    let result = tokio::select! {
      _ = crate::utils::shutdown_signal() => { cancelled = true; Ok(()) },
      result = async {
      let mut minimized = self.minimized;
      loop {
        let credential = read_credential(&mut output).await?;
        let Some(credential) = credential else { return Ok(()); };
        received_credential = true;
        ensure!(credential.product_version() == self.version, "Service product version mismatch");
        let mut envs: HashMap<String, String> = std::env::vars().collect();
        if let Some(path) = &self.log_file {
          envs.insert("GP_LOG_FILE".into(), path.clone());
        }
        let launcher = GuiLauncher::new(self.version, Arc::new(credential)).envs(envs).minimized(minimized);
        let mut unexpected = [0];
        tokio::select! {
          result = launcher.launch() => { result?; }
          status = service.wait() => { anyhow::bail!("The privileged service exited while the GUI was running: {}", status?); }
          result = output.read(&mut unexpected) => {
            ensure!(result? == 0, "Unexpected service credential data while the GUI is running");
            anyhow::bail!("The privileged service closed its credential channel");
          }
        }
        input.write_all(&[0]).await?;
        minimized = false;
      }
      } => result,
    };
    // EOF lets the service revoke the credential and disconnect the VPN gracefully.
    drop(input);
    if cancelled && !received_credential {
      // pkexec is still user-owned during authorization. After elevation, EOF owns shutdown.
      if let Err(error) = service.start_kill()
        && error.kind() != std::io::ErrorKind::PermissionDenied
      {
        return Err(error.into());
      }
    }
    let status = service.wait().await?;
    result?;
    ensure!(
      cancelled || status.success(),
      "The privileged service exited with {status}"
    );
    Ok(status)
  }
}

async fn read_credential(reader: &mut (impl AsyncRead + Unpin)) -> anyhow::Result<Option<SessionCredential>> {
  let mut header = [0; 2];
  if reader.read(&mut header[..1]).await? == 0 {
    return Ok(None);
  }
  reader.read_exact(&mut header[1..]).await?;
  let length = usize::from(u16::from_be_bytes(header));
  ensure!(
    length != 0 && length + 2 <= MAX_CREDENTIAL_FRAME,
    "Invalid service credential length"
  );
  let mut frame = Zeroizing::new(vec![0; length + 2]);
  frame[..2].copy_from_slice(&header);
  reader.read_exact(&mut frame[2..]).await?;
  Ok(Some(SessionCredential::decode_frame(&frame)?))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn credential_channel_distinguishes_clean_close_from_truncated_or_oversized_frames() {
    assert!(read_credential(&mut &[][..]).await.unwrap().is_none());
    assert!(read_credential(&mut &[0][..]).await.is_err());
    assert!(read_credential(&mut &[0, 0][..]).await.is_err());
    assert!(read_credential(&mut &[255, 255][..]).await.is_err());
    let credential = SessionCredential::generate(uuid::Uuid::new_v4(), "test").unwrap();
    let frame = credential.encode_frame().unwrap();
    let decoded = read_credential(&mut frame.as_slice()).await.unwrap().unwrap();
    assert_eq!(decoded.session_id(), credential.session_id());
  }
}
