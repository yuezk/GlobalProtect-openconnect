use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};

use gptunnel::TunnelMonitor;
use tokio_util::sync::CancellationToken;

use crate::{
  background::{BackgroundStdio, BackgroundWorker},
  dashboard::Dashboard,
};

use super::ConnectHandler;

pub(super) struct TunnelOutput<'a> {
  background: Option<(&'a BackgroundWorker, BackgroundStdio)>,
  dashboard: Option<Dashboard>,
  pid_written: &'a AtomicBool,
  cancellation: &'a CancellationToken,
  error: Option<anyhow::Error>,
}

impl<'a> TunnelOutput<'a> {
  pub(super) fn prepare(handler: &'a ConnectHandler<'_>, gateway: &str) -> anyhow::Result<Self> {
    let background = handler
      .background
      .as_ref()
      .map(|worker| worker.redirect_stdio().map(|stdio| (worker, stdio)))
      .transpose()?;
    let dashboard = handler
      .args
      .dashboard
      .then(|| Dashboard::prepare(gateway.to_owned(), handler.cancellation.clone()))
      .transpose()?;
    Ok(Self {
      background,
      dashboard,
      pid_written: &handler.pid_written,
      cancellation: &handler.cancellation,
      error: None,
    })
  }

  pub(super) fn ready(&mut self, monitor: Arc<TunnelMonitor>) {
    if let Some(dashboard) = &mut self.dashboard
      && let Err(error) = dashboard.start_tunnel(monitor)
    {
      self.cancellation.cancel();
      self.error = Some(error);
    }
    if let Some((worker, stdio)) = &mut self.background {
      let detach = if self.pid_written.load(Ordering::SeqCst) {
        worker.detach(stdio)
      } else {
        Err(anyhow::anyhow!("Cannot detach without a writable PID lock file"))
      };
      if let Err(error) = detach {
        self.cancellation.cancel();
        self.error = Some(error);
      }
    }
  }

  pub(super) fn finish(&mut self) -> anyhow::Result<()> {
    if let Some(dashboard) = &mut self.dashboard {
      dashboard.finish()?;
    }
    match self.error.take() {
      Some(error) => Err(error),
      None => Ok(()),
    }
  }
}
