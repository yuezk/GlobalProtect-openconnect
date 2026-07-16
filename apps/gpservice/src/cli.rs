#[cfg(debug_assertions)]
use std::path::PathBuf;
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};
use std::{collections::HashMap, io::Write};

use anyhow::{Context, bail};
use clap::Parser;
use common::constants::GP_SERVICE_LOCK_FILE;
use gpapi::clap::InfoLevelVerbosity;
use gpapi::logger;
use gpapi::{
  process::gui_launcher::GuiLauncher,
  service::{request::WsRequest, vpn_state::VpnState},
  utils::{env_utils, lock_file::LockFile, redact::Redaction, shutdown_signal},
};
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
  #[command(flatten)]
  verbose: InfoLevelVerbosity,
}

impl Cli {
  async fn run(&mut self) -> anyhow::Result<()> {
    let redaction = self.init_logger();
    info!("gpservice started: {}", VERSION);

    let pid = std::process::id();
    let lock_file = Arc::new(LockFile::new(GP_SERVICE_LOCK_FILE, pid));

    if lock_file.check_health().await {
      bail!("Another instance of the service is already running");
    }

    // Channel for sending requests to the VPN task
    let (ws_req_tx, ws_req_rx) = mpsc::channel::<WsRequest>(32);
    // Channel for receiving the VPN state from the VPN task
    let (vpn_state_tx, vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let gui_restart_requested = Arc::new(AtomicBool::new(false));
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let dispatcher = Arc::new(RequestDispatcher::new(
      ws_req_tx.clone(),
      Arc::clone(&gui_restart_requested),
      Arc::clone(&redaction),
    ));

    let mut vpn_task = VpnTask::new(ws_req_rx, vpn_state_tx);
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
    let launch_managed_gui = !self.dev_standalone;
    #[cfg(not(debug_assertions))]
    let launch_managed_gui = true;

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

  fn init_logger(&self) -> Arc<Redaction> {
    let redaction = Arc::new(Redaction::new());
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

    let level = self.verbose.log_level_filter().to_level().unwrap_or(log::Level::Info);

    logger::init_with_logger(level, inner_logger);

    redaction
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
