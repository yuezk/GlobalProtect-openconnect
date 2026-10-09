use std::{
  io,
  os::fd::OwnedFd,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
};

use anyhow::{Context, ensure};
use nix::{
  fcntl::{FcntlArg, FdFlag, OFlag, fcntl},
  sys::stat::{SFlag, fstat},
  unistd::{dup, read, write},
};
use tokio::io::unix::AsyncFd;
use tokio_util::sync::CancellationToken;

use crate::session_registry::SessionRegistry;

/// Private parent/service pipes; credentials never pass through argv, environment or files.
pub(crate) struct DesktopClient {
  input: AsyncFd<OwnedFd>,
  output: AsyncFd<OwnedFd>,
}

impl DesktopClient {
  pub(crate) fn from_stdio() -> anyhow::Result<Self> {
    let channel = Self::new(dup(std::io::stdin())?, dup(std::io::stdout())?)?;
    // Only the owned CLOEXEC descriptors carry credentials. VPN subprocesses inherit /dev/null.
    let null = std::fs::OpenOptions::new().read(true).write(true).open("/dev/null")?;
    nix::unistd::dup2_stdin(&null)?;
    nix::unistd::dup2_stdout(&null)?;
    Ok(channel)
  }

  fn new(input: OwnedFd, output: OwnedFd) -> anyhow::Result<Self> {
    fn pipe(fd: OwnedFd) -> anyhow::Result<AsyncFd<OwnedFd>> {
      ensure!(
        SFlag::from_bits_truncate(fstat(&fd)?.st_mode).contains(SFlag::S_IFIFO),
        "Desktop credentials require private pipes"
      );
      fcntl(&fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
      let flags = OFlag::from_bits_truncate(fcntl(&fd, FcntlArg::F_GETFL)?);
      fcntl(&fd, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
      Ok(AsyncFd::new(fd)?)
    }
    Ok(Self {
      input: pipe(input)?,
      output: pipe(output)?,
    })
  }

  pub(crate) async fn serve(
    self,
    registry: Arc<SessionRegistry>,
    uid: u32,
    restart: Arc<AtomicBool>,
    cancel: CancellationToken,
  ) -> anyhow::Result<()> {
    loop {
      let credential = registry.issue(env!("CARGO_PKG_VERSION"), Some(uid))?;
      let session_id = credential.session_id();
      let result = tokio::select! {
        _ = cancel.cancelled() => Ok(false),
        result = async {
          self.write_frame(&credential.encode_frame()?).await?;
          self.gui_exited().await
        } => result,
      };
      registry.revoke(session_id);
      if !result? || !restart.swap(false, Ordering::SeqCst) {
        return Ok(());
      }
    }
  }

  async fn write_frame(&self, mut bytes: &[u8]) -> anyhow::Result<()> {
    while !bytes.is_empty() {
      let mut ready = self.output.writable().await?;
      match ready.try_io(|fd| write(fd.get_ref(), bytes).map_err(io::Error::from)) {
        Ok(result) => {
          let length = result?;
          ensure!(length != 0, "Desktop credential pipe closed");
          bytes = &bytes[length..];
        }
        Err(_) => continue,
      }
    }
    Ok(())
  }

  async fn gui_exited(&self) -> anyhow::Result<bool> {
    loop {
      let mut ready = self.input.readable().await?;
      let mut byte = [0];
      match ready.try_io(|fd| read(fd.get_ref(), &mut byte).map_err(io::Error::from)) {
        Ok(result) => {
          if result.context("Desktop control pipe failed")? == 0 {
            return Ok(false);
          }
          ensure!(byte[0] == 0, "Invalid desktop control message");
          return Ok(true);
        }
        Err(_) => continue,
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn credential_stdio_is_not_inherited_by_subprocesses() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
      .args(["--ignored", "--exact", "desktop_client::tests::stdio_child"])
      .stdin(std::process::Stdio::piped())
      .output()
      .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("PRIVATE_FRAME"));
    assert!(!text.contains("CHILD_OUTPUT"));
  }

  #[tokio::test]
  #[ignore = "subprocess fixture for credential_stdio_is_not_inherited_by_subprocesses"]
  async fn stdio_child() {
    let channel = DesktopClient::from_stdio().unwrap();
    assert!(
      std::process::Command::new("/bin/sh")
        .args(["-c", "printf CHILD_OUTPUT"])
        .status()
        .unwrap()
        .success()
    );
    channel.write_frame(b"PRIVATE_FRAME").await.unwrap();
  }
  use gpapi::service::transport::SessionCredential;
  use nix::unistd::pipe;
  use std::{fs::File, io::Read, time::Duration};

  fn pipes() -> (DesktopClient, OwnedFd, File) {
    let (input, control) = pipe().unwrap();
    let (credentials, output) = pipe().unwrap();
    (DesktopClient::new(input, output).unwrap(), control, credentials.into())
  }

  async fn credential(mut reader: File) -> (File, SessionCredential) {
    tokio::task::spawn_blocking(move || {
      let mut header = [0; 2];
      reader.read_exact(&mut header).unwrap();
      let mut frame = zeroize::Zeroizing::new(vec![0; usize::from(u16::from_be_bytes(header)) + 2]);
      frame[..2].copy_from_slice(&header);
      reader.read_exact(&mut frame[2..]).unwrap();
      let credential = SessionCredential::decode_frame(&frame).unwrap();
      (reader, credential)
    })
    .await
    .unwrap()
  }

  #[tokio::test]
  async fn desktop_channel_revokes_and_reissues_on_restart_then_closes_on_parent_exit() {
    let (channel, control, reader) = pipes();
    let registry = Arc::new(SessionRegistry::new(uuid::Uuid::new_v4()));
    let restart = Arc::new(AtomicBool::new(false));
    let task = tokio::spawn(channel.serve(registry.clone(), 1000, restart.clone(), CancellationToken::new()));
    let (reader, first) = credential(reader).await;
    restart.store(true, Ordering::SeqCst);
    write(&control, &[0]).unwrap();
    let (_, second) = credential(reader).await;
    assert_ne!(first.session_id(), second.session_id());
    assert!(registry.last_successful_hip_report(first.session_id()).is_err());
    drop(control);
    tokio::time::timeout(Duration::from_secs(1), task)
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    assert!(registry.last_successful_hip_report(second.session_id()).is_err());
  }

  #[tokio::test]
  async fn desktop_channel_shutdown_cancels_pipe_wait_and_revokes_credential() {
    let (channel, _control, reader) = pipes();
    let registry = Arc::new(SessionRegistry::new(uuid::Uuid::new_v4()));
    let cancel = CancellationToken::new();
    let task = tokio::spawn(channel.serve(registry.clone(), 1000, Arc::new(AtomicBool::new(false)), cancel.clone()));
    let (_, issued) = credential(reader).await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    assert!(registry.last_successful_hip_report(issued.session_id()).is_err());
  }

  #[tokio::test]
  async fn desktop_channel_rejects_invalid_control_messages_and_revokes() {
    let (channel, control, reader) = pipes();
    let registry = Arc::new(SessionRegistry::new(uuid::Uuid::new_v4()));
    let task = tokio::spawn(channel.serve(
      registry.clone(),
      1000,
      Arc::new(AtomicBool::new(false)),
      CancellationToken::new(),
    ));
    let (_, issued) = credential(reader).await;
    write(&control, &[1]).unwrap();
    assert!(
      tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .is_err()
    );
    assert!(registry.last_successful_hip_report(issued.session_id()).is_err());
  }
}
