//! Optional foreground presentation. Connection ownership stays with ConnectHandler.
mod layout;
mod notice;
mod output;
mod restoration;
mod screen;
#[cfg(test)]
mod tests;

use std::{
  fs::{File, OpenOptions},
  io::IsTerminal,
  os::fd::AsRawFd,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
  },
  thread::JoinHandle,
};

use anyhow::{Context, ensure};
use gpapi::log_format::LogFormat;
use nix::unistd::{getpgrp, tcgetpgrp};
use tokio_util::sync::CancellationToken;

pub(crate) use notice::{NoticeLogger, session_notice};
use output::OutputRouting;

pub(crate) type InternalMembers = Arc<Mutex<std::collections::BTreeMap<String, String>>>;

pub(crate) fn validate(format: LogFormat) -> anyhow::Result<()> {
  ensure!(
    format == LogFormat::Text,
    "--dashboard conflicts with --log-format json"
  );
  ensure!(
    std::env::var_os("TERM").is_some_and(|term| term != "dumb"),
    "--dashboard requires a capable terminal (TERM must not be dumb)"
  );
  let tty = terminal()?;
  ensure!(
    std::io::stderr().is_terminal(),
    "--dashboard requires stderr attached to the controlling terminal"
  );
  let stderr = output::duplicate(2)?;
  ensure!(
    same_terminal(&stderr, &tty),
    "--dashboard requires stderr attached to the controlling terminal"
  );
  ensure!(
    tcgetpgrp(&tty)? == getpgrp(),
    "--dashboard requires a foreground terminal"
  );
  Ok(())
}

fn terminal() -> anyhow::Result<File> {
  OpenOptions::new()
    .read(true)
    .write(true)
    .open("/dev/tty")
    .context("--dashboard requires a controlling terminal")
}

fn same_terminal(first: &File, second: &File) -> bool {
  // /dev/tty is a multiplexor node, so its st_rdev differs from the underlying
  // PTY. A session has one controlling terminal; compare terminal session IDs.
  let first_session = unsafe { nix::libc::tcgetsid(first.as_raw_fd()) };
  let second_session = unsafe { nix::libc::tcgetsid(second.as_raw_fd()) };
  first_session >= 0 && first_session == second_session
}

pub(super) enum Content {
  Tunnel(Arc<gptunnel::TunnelMonitor>),
  Internal(InternalMembers),
}

pub(super) enum Command {
  Suspend,
  Resume,
}

pub(crate) struct Dashboard {
  routing: OutputRouting,
  server: String,
  cancellation: CancellationToken,
  stop: Arc<AtomicBool>,
  worker: Option<JoinHandle<()>>,
  signals: Vec<tokio::task::JoinHandle<()>>,
  error: Arc<Mutex<Option<String>>>,
}

impl Dashboard {
  /// Install before native startup so persistent children inherit captured descriptors.
  pub(crate) fn prepare(server: String, cancellation: CancellationToken) -> anyhow::Result<Self> {
    let capture_stdout =
      std::io::stdout().is_terminal() && same_terminal(&output::duplicate(1)?, &output::duplicate(2)?);
    Ok(Self {
      routing: OutputRouting::install(capture_stdout, cancellation.clone())?,
      server,
      cancellation,
      stop: Arc::default(),
      worker: None,
      signals: Vec::new(),
      error: Arc::default(),
    })
  }

  pub(crate) fn start_tunnel(&mut self, monitor: Arc<gptunnel::TunnelMonitor>) -> anyhow::Result<()> {
    self.start(Content::Tunnel(monitor.clone()))?;
    monitor.set_statistics_enabled(true);
    Ok(())
  }

  pub(crate) fn start_internal(&mut self, members: InternalMembers) -> anyhow::Result<()> {
    self.start(Content::Internal(members))
  }

  fn start(&mut self, content: Content) -> anyhow::Result<()> {
    ensure!(self.worker.is_none(), "Dashboard already started");
    let (commands, receiver) = mpsc::channel();
    for (kind, command) in [
      (
        tokio::signal::unix::SignalKind::from_raw(nix::libc::SIGTSTP),
        Command::Suspend,
      ),
      (
        tokio::signal::unix::SignalKind::from_raw(nix::libc::SIGCONT),
        Command::Resume,
      ),
    ] {
      let mut signal = tokio::signal::unix::signal(kind)?;
      let commands = commands.clone();
      self.signals.push(tokio::spawn(async move {
        while signal.recv().await.is_some() {
          let command = match command {
            Command::Suspend => Command::Suspend,
            Command::Resume => Command::Resume,
          };
          if commands.send(command).is_err() {
            break;
          }
        }
      }));
    }
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let cancellation = self.cancellation.clone();
    self.signals.push(tokio::spawn(async move {
      if hangup.recv().await.is_some() {
        cancellation.cancel();
      }
    }));
    let (ready, started) = mpsc::sync_channel(1);
    let context = screen::Context {
      terminal: terminal()?,
      output: self.routing.original_error.clone(),
      saved_stdout: self.routing.saved_output(),
      saved_stdin_flags: self.routing.saved_input_flags(),
      history: self.routing.history(),
      passthrough: self.routing.passthrough(),
      gate: self.routing.gate(),
      stop: self.stop.clone(),
      cancellation: self.cancellation.clone(),
      notice: notice::slot(),
      server: self.server.clone(),
      content,
      commands: receiver,
    };
    let error = self.error.clone();
    let cancellation = self.cancellation.clone();
    self.worker = Some(std::thread::Builder::new().name("dashboard".into()).spawn(move || {
      let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| screen::run(context, ready)));
      let failure = match result {
        Ok(Ok(())) => None,
        Ok(Err(failure)) => Some(format!("{failure:#}")),
        Err(_) => Some("Dashboard renderer panicked".into()),
      };
      if let Some(failure) = failure {
        *error.lock().unwrap_or_else(|error| error.into_inner()) = Some(failure);
        cancellation.cancel();
      }
    })?);
    started.recv().context("Dashboard stopped during initialization")??;
    Ok(())
  }

  /// Called after native shutdown and network cleanup, while capture still drains.
  pub(crate) fn finish(&mut self) -> anyhow::Result<()> {
    self.stop.store(true, Ordering::Release);
    if let Some(worker) = self.worker.take() {
      let _ = worker.join();
    }
    for signal in self.signals.drain(..) {
      signal.abort();
    }
    self.routing.restore();
    if let Some(error) = self.routing.take_error() {
      anyhow::bail!("Dashboard failed: {error}");
    }
    if let Some(error) = self.error.lock().unwrap_or_else(|error| error.into_inner()).take() {
      anyhow::bail!("Dashboard failed: {error}");
    }
    Ok(())
  }
}

impl Drop for Dashboard {
  fn drop(&mut self) {
    let _ = self.finish();
  }
}
