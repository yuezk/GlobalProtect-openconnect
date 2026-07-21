use std::path::PathBuf;
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};
#[cfg(target_os = "macos")]
use std::time::Duration;
use std::{collections::HashMap, io::Write};

use anyhow::{Context, bail};
use clap::Parser;
use common::constants::GP_SERVICE_LOCK_FILE;
use gpapi::clap::InfoLevelVerbosity;
use gpapi::logger;
#[cfg(debug_assertions)]
use gpapi::utils::lock_file::dev_service_lock_file_path;
use gpapi::{
  process::gui_launcher::GuiLauncher,
  service::{request::WsRequest, vpn_state::VpnState},
  utils::{env_utils, lock_file::LockFile, redact::Redaction, shutdown_signal},
};
#[cfg(target_os = "macos")]
use log::{Log, Metadata, Record};
use log::{info, warn};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::{
  request_dispatcher::RequestDispatcher, session_registry::SessionRegistry, vpn_task::VpnTask, ws_server::WsServer,
};

const VERSION: &str = concat!(
  env!("CARGO_PKG_VERSION"),
  " (",
  env!("GPSERVICE_GIT_COMMIT"),
  " ",
  compile_time::date_str!(),
  ")"
);

#[cfg(target_os = "macos")]
struct RedactingMacosLogger {
  logger: logger::MacosLogger,
  redaction: Arc<Redaction>,
}

#[cfg(target_os = "macos")]
impl Log for RedactingMacosLogger {
  fn enabled(&self, metadata: &Metadata<'_>) -> bool {
    self.logger.enabled(metadata)
  }

  fn log(&self, record: &Record<'_>) {
    if self.enabled(record.metadata()) {
      let message = self.redaction.redact_str(&record.args().to_string());
      self.logger.write(record.level(), record.target(), &message);
    }
  }

  fn flush(&self) {}
}

#[derive(Parser)]
#[command(version = VERSION)]
struct Cli {
  #[clap(long)]
  minimized: bool,
  #[clap(long)]
  env_file: Option<String>,
  #[cfg(debug_assertions)]
  #[clap(long, requires_all = ["dev_uid", "dev_bootstrap_socket"])]
  dev_standalone: bool,
  #[cfg(debug_assertions)]
  #[clap(long, requires = "dev_standalone")]
  dev_uid: Option<u32>,
  #[cfg(debug_assertions)]
  #[clap(long, requires = "dev_standalone")]
  dev_bootstrap_socket: Option<PathBuf>,
  #[cfg(target_os = "macos")]
  #[clap(long, conflicts_with = "dev_standalone")]
  macos_brokered: bool,
  #[command(flatten)]
  verbose: InfoLevelVerbosity,
}

impl Cli {
  async fn run(&mut self) -> anyhow::Result<()> {
    let redaction = self.init_logger();
    info!("gpservice started: {}", VERSION);

    let pid = std::process::id();
    let lock_file = Arc::new(LockFile::new(self.service_lock_file_path()?, pid));

    if lock_file.check_health().await {
      bail!("Another instance of the service is already running");
    }

    // Channel for sending requests to the VPN task
    let (ws_req_tx, ws_req_rx) = mpsc::channel::<WsRequest>(32);
    // Channel for receiving the VPN state from the VPN task
    let (vpn_state_tx, vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let gui_restart_requested = Arc::new(AtomicBool::new(false));
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let externally_brokered = self.externally_brokered();
    let brokered_scripts_dir = if externally_brokered {
      let executable = std::env::current_exe().context("Failed to locate the gpservice executable")?;
      let contents_dir = executable
        .parent()
        .and_then(std::path::Path::parent)
        .context("gpservice is not inside a macOS application bundle")?;
      Some(contents_dir.join("Resources/Scripts"))
    } else {
      None
    };
    let dispatcher = Arc::new(RequestDispatcher::new(
      ws_req_tx.clone(),
      Arc::clone(&gui_restart_requested),
      Arc::clone(&redaction),
      !externally_brokered,
      brokered_scripts_dir,
    ));

    let mut vpn_task = VpnTask::new(ws_req_rx, vpn_state_tx, externally_brokered);
    let idle_vpn_state_rx = vpn_state_rx.clone();
    let ws_server = WsServer::new(
      env!("CARGO_PKG_VERSION"),
      Arc::clone(&registry),
      dispatcher,
      vpn_state_rx,
      lock_file.clone(),
    );

    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(4);
    let shutdown_tx_clone = shutdown_tx.clone();
    let vpn_task_cancel_token = vpn_task.cancel_token();
    let server_token = ws_server.cancel_token();

    #[cfg(target_os = "macos")]
    if externally_brokered {
      let credential_server = crate::macos_broker::CredentialServer::new(Arc::clone(&registry));
      let credential_token = server_token.clone();
      let credential_shutdown = shutdown_tx.clone();
      tokio::spawn(async move {
        if let Err(err) = credential_server.start(credential_token).await {
          warn!("Broker credential server stopped: {err}");
          let _ = credential_shutdown.send(()).await;
        }
      });

      let idle_registry = Arc::clone(&registry);
      let idle_shutdown = shutdown_tx.clone();
      let idle_token = server_token.clone();
      tokio::spawn(async move {
        wait_for_brokered_idle(idle_vpn_state_rx, idle_registry, idle_token).await;
        let _ = idle_shutdown.send(()).await;
      });
    }

    #[cfg(debug_assertions)]
    if self.dev_standalone {
      let Some(dev_bootstrap_socket) = self.dev_bootstrap_socket.clone() else {
        bail!("--dev-bootstrap-socket is required with --dev-standalone");
      };
      let Some(dev_uid) = self.dev_uid else {
        bail!("--dev-uid is required with --dev-standalone");
      };
      let bootstrap = crate::dev_bootstrap::DevBootstrap::new(dev_bootstrap_socket, dev_uid, Arc::clone(&registry));
      let bootstrap_token = server_token.clone();
      let bootstrap_shutdown = shutdown_tx.clone();
      tokio::spawn(async move {
        if let Err(err) = bootstrap.start(bootstrap_token).await {
          warn!("Debug bootstrap stopped: {err}");
          let _ = bootstrap_shutdown.send(()).await;
        }
      });
    }

    #[cfg(unix)]
    {
      let vpn_ctx = vpn_task.context();
      let ws_ctx = ws_server.context();

      tokio::spawn(async move { signals::handle_signals(vpn_ctx, ws_ctx).await });
    }

    let vpn_task_handle = tokio::spawn(async move { vpn_task.start(server_token).await });
    let (ws_ready_tx, ws_ready_rx) = tokio::sync::oneshot::channel();
    let ws_server_handle = tokio::spawn(async move { ws_server.start(shutdown_tx_clone, ws_ready_tx).await });

    #[cfg(debug_assertions)]
    let launch_managed_gui = !self.dev_standalone && !externally_brokered;
    #[cfg(not(debug_assertions))]
    let launch_managed_gui = !externally_brokered;

    if launch_managed_gui {
      ws_ready_rx.await.context("WebSocket server failed to start")?;
      let envs = self.env_file.as_ref().map(env_utils::load_env_vars).transpose()?;
      let minimized = self.minimized;
      tokio::spawn(async move {
        launch_gui(envs, registry, minimized, gui_restart_requested).await;
        let _ = shutdown_tx.send(()).await;
      });
    } else {
      info!("Running with a separately started debug GUI");
    }

    tokio::select! {
      _ = shutdown_signal() => {
        info!("Shutdown signal received");
      }
      _ = shutdown_rx.recv() => {
        info!("Shutdown request received, shutting down");
      }
    }

    vpn_task_cancel_token.cancel();
    let _ = tokio::join!(vpn_task_handle, ws_server_handle);

    lock_file.unlock()?;

    info!("gpservice stopped");

    Ok(())
  }

  fn externally_brokered(&self) -> bool {
    #[cfg(target_os = "macos")]
    return self.macos_brokered;

    #[cfg(not(target_os = "macos"))]
    false
  }

  fn service_lock_file_path(&self) -> anyhow::Result<PathBuf> {
    #[cfg(debug_assertions)]
    if self.dev_standalone {
      let socket = self
        .dev_bootstrap_socket
        .as_ref()
        .context("--dev-bootstrap-socket is required with --dev-standalone")?;
      return Ok(dev_service_lock_file_path(socket));
    }

    Ok(PathBuf::from(GP_SERVICE_LOCK_FILE))
  }

  fn init_logger(&self) -> Arc<Redaction> {
    let redaction = Arc::new(Redaction::new());
    let level = self.verbose.log_level_filter().to_level().unwrap_or(log::Level::Info);

    #[cfg(target_os = "macos")]
    if logger::macos_unified_log_enabled() {
      logger::init_with_logger(
        level,
        RedactingMacosLogger {
          logger: logger::MacosLogger::new("com.yuezk.gpgui", "gpservice"),
          redaction: Arc::clone(&redaction),
        },
      );
      return redaction;
    }

    let redaction_clone = Arc::clone(&redaction);

    let inner_logger = env_logger::builder()
      // Set the log level to the Trace level, the logs will be filtered
      .filter_level(log::LevelFilter::Trace)
      .format(move |buf, record| {
        let timestamp = buf.timestamp();
        writeln!(
          buf,
          "[{} {:<5} {}] {}",
          timestamp,
          record.level(),
          record.module_path().unwrap_or_default(),
          redaction_clone.redact_str(&record.args().to_string())
        )
      })
      .build();

    logger::init_with_logger(level, inner_logger);

    redaction
  }
}

#[cfg(target_os = "macos")]
async fn wait_for_brokered_idle(
  mut vpn_state_rx: watch::Receiver<VpnState>,
  registry: Arc<SessionRegistry>,
  cancel: tokio_util::sync::CancellationToken,
) {
  const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
  const CHECK_INTERVAL: Duration = Duration::from_secs(1);

  let mut idle_since = None;
  let mut interval = tokio::time::interval(CHECK_INTERVAL);
  loop {
    tokio::select! {
      _ = cancel.cancelled() => return,
      _ = vpn_state_rx.changed() => {},
      _ = interval.tick() => {},
    }

    let idle = matches!(*vpn_state_rx.borrow(), VpnState::Disconnected) && registry.is_empty();
    if !idle {
      idle_since = None;
      continue;
    }

    let started = idle_since.get_or_insert_with(tokio::time::Instant::now);
    if started.elapsed() >= IDLE_TIMEOUT {
      info!("Brokered service idle timeout reached");
      return;
    }
  }
}

#[cfg(unix)]
mod signals {
  use std::sync::Arc;

  use log::{info, warn};

  use crate::vpn_task::VpnTaskContext;
  use crate::ws_server::WsServerContext;

  const DISCONNECTED_PID_FILE: &str = "/tmp/gpservice_disconnected.pid";

  pub async fn handle_signals(vpn_ctx: Arc<VpnTaskContext>, ws_ctx: Arc<WsServerContext>) {
    use gpapi::service::event::WsEvent;
    use tokio::signal::unix::{Signal, SignalKind, signal};

    let (mut user_sig1, mut user_sig2) = match || -> anyhow::Result<(Signal, Signal)> {
      let user_sig1 = signal(SignalKind::user_defined1())?;
      let user_sig2 = signal(SignalKind::user_defined2())?;
      Ok((user_sig1, user_sig2))
    }() {
      Ok(signals) => signals,
      Err(err) => {
        warn!("Failed to create signal: {}", err);
        return;
      }
    };

    loop {
      tokio::select! {
        _ = user_sig1.recv() => {
          info!("Received SIGUSR1 signal");
          if vpn_ctx.disconnect().await {
            // Write the PID to a dedicated file to indicate that the VPN task is disconnected via SIGUSR1
            let pid = std::process::id();
            if let Err(err) = tokio::fs::write(DISCONNECTED_PID_FILE, pid.to_string()).await {
              warn!("Failed to write PID to file: {}", err);
            }
          }
        }
        _ = user_sig2.recv() => {
          info!("Received SIGUSR2 signal");
          ws_ctx.send_event(WsEvent::ResumeConnection).await;
        }
      }
    }
  }
}

async fn launch_gui(
  envs: Option<HashMap<String, String>>,
  registry: Arc<SessionRegistry>,
  mut minimized: bool,
  restart_requested: Arc<AtomicBool>,
) {
  loop {
    let credential = match registry.issue(env!("CARGO_PKG_VERSION")) {
      Ok(credential) => credential,
      Err(err) => {
        warn!("Failed to issue GUI credential: {err}");
        break;
      }
    };
    let session_id = credential.session_id();
    let gui_launcher = GuiLauncher::new(env!("CARGO_PKG_VERSION"), credential)
      .envs(envs.clone())
      .minimized(minimized);

    match gui_launcher.launch().await {
      Ok(exit_status) => {
        registry.revoke(session_id);
        let should_restart = restart_requested.swap(false, Ordering::SeqCst);
        if !should_restart {
          info!("GUI exited with code {:?}", exit_status.code());
          break;
        }

        info!("GUI restart requested, restarting");
        minimized = false;
      }
      Err(err) => {
        registry.revoke(session_id);
        warn!("Failed to launch GUI: {}", err);
        break;
      }
    }
  }
}

pub async fn run() {
  let mut cli = Cli::parse();

  if let Err(e) = cli.run().await {
    eprintln!("Error: {}", e);
    std::process::exit(1);
  }
}
