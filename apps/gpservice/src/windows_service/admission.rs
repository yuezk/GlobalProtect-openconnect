//! A credential lease binds the existing desktop session to its Windows token.
use crate::session_registry::SessionRegistry;
use anyhow::{Context, ensure};
use gpapi::{
  service::transport::WINDOWS_ADMISSION_REQUEST,
  windows::{PipeClientIdentity, create_service_admission_pipe},
};
use std::{sync::Arc, time::Duration};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::windows::named_pipe::NamedPipeServer,
  task::JoinSet,
};
use tokio_util::sync::CancellationToken;

pub(super) struct Admission {
  listener: NamedPipeServer,
  registry: Arc<SessionRegistry>,
}
struct Lease {
  registry: Arc<SessionRegistry>,
  session: uuid::Uuid,
}
impl Drop for Lease {
  fn drop(&mut self) {
    self.registry.revoke(self.session);
  }
}
impl Admission {
  pub fn new(registry: Arc<SessionRegistry>) -> anyhow::Result<Self> {
    Ok(Self {
      listener: create_service_admission_pipe(true)?,
      registry,
    })
  }
  pub async fn run(mut self, cancel: CancellationToken) -> anyhow::Result<()> {
    let mut clients = JoinSet::new();
    let result = loop {
      tokio::select! {
        _ = cancel.cancelled() => break Ok(()),
        result = self.listener.connect() => {
          if let Err(error) = result { break Err(error.into()); }
          let next = match create_service_admission_pipe(false) { Ok(next) => next, Err(error) => break Err(error.into()) };
          let pipe = std::mem::replace(&mut self.listener, next);
          let registry = self.registry.clone();
          let cancel = cancel.clone();
          clients.spawn(async move {
            if let Err(error) = serve(pipe, registry, cancel).await { log::warn!("Desktop credential lease ended: {error:#}"); }
          });
        }
        _ = clients.join_next(), if !clients.is_empty() => {},
      }
    };
    // Drop each lease before the shared registry and service runtime disappear.
    clients.abort_all();
    while clients.join_next().await.is_some() {}
    result
  }
}
async fn serve(
  mut pipe: NamedPipeServer,
  registry: Arc<SessionRegistry>,
  cancel: CancellationToken,
) -> anyhow::Result<()> {
  let mut request = [0; WINDOWS_ADMISSION_REQUEST.len()];
  tokio::select! {
    _ = cancel.cancelled() => return Ok(()),
    result = tokio::time::timeout(Duration::from_secs(10), pipe.read_exact(&mut request)) => { result.context("Desktop admission timed out")??; }
  }
  ensure!(
    request == WINDOWS_ADMISSION_REQUEST,
    "Invalid desktop admission request"
  );
  // This synchronous call impersonates, duplicates and reverts before any await.
  let identity = Arc::new(PipeClientIdentity::from_pipe(&pipe)?);
  let credential = registry.issue_windows(env!("CARGO_PKG_VERSION"), identity)?;
  let _lease = Lease {
    registry,
    session: credential.session_id(),
  };
  let frame = credential.encode_frame()?;
  tokio::select! {
    _ = cancel.cancelled() => return Ok(()),
    result = tokio::time::timeout(Duration::from_secs(10), pipe.write_all(&frame)) => { result.context("Desktop credential delivery timed out")??; }
  }
  let mut byte = [0];
  tokio::select! {
    _ = cancel.cancelled() => {},
    result = pipe.read(&mut byte) => { ensure!(result? == 0, "Unexpected desktop lease message"); }
  }
  Ok(())
}
