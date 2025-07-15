use std::sync::Arc;
use std::{collections::HashMap, io::Write};

use anyhow::bail;
use clap::Parser;
use gpapi::clap::InfoLevelVerbosity;
use gpapi::logger;
use gpapi::{
  process::gui_launcher::GuiLauncher,
  service::{request::WsRequest, vpn_state::VpnState},
  utils::{crypto::generate_key, env_utils, lock_file::LockFile, redact::Redaction, runtime, shutdown_signal},
};
use log::{info, warn};
use tokio::sync::{mpsc, watch};

use crate::{vpn_task::VpnTask, ws_server::WsServer};

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", compile_time::date_str!(), ")");

#[derive(Parser)]
#[command(version = VERSION)]
struct Cli {
  #[clap(long)]
  minimized: bool,
  #[clap(long)]
  env_file: Option<String>,
  #[cfg(debug_assertions)]
  #[clap(long)]
  no_gui: bool,

  #[command(flatten)]
  verbose: InfoLevelVerbosity,
}

impl Cli {
  async fn run(&mut self) -> anyhow::Result<()> {
    let redaction = self.init_logger();
    info!("gpservice started: {}", VERSION);

    let pid = std::process::id();

    // Determine appropriate lock file path based on user privileges
    let lock_file_path =
      runtime::get_service_lock_path().map_err(|e| anyhow::anyhow!("Failed to determine lock file path: {}", e))?;

    info!("Using lock file: {}", lock_file_path.display());

    // Check if we have permission to use this lock file path
    if let Err(e) = runtime::ensure_lock_file_accessible(&lock_file_path) {
      bail!("Cannot access lock file: {}", e);
    }

    let lock_file = Arc::new(LockFile::new(lock_file_path, pid));

    if lock_file.check_health().await {
      bail!("Another instance of the service is already running");
    }

    let api_key = self.prepare_api_key();

    // Channel for sending requests to the VPN task
    let (ws_req_tx, ws_req_rx) = mpsc::channel::<WsRequest>(32);
    // Channel for receiving the VPN state from the VPN task
    let (vpn_state_tx, vpn_state_rx) = watch::channel(VpnState::Disconnected);

    let mut vpn_task = VpnTask::new(ws_req_rx, vpn_state_tx);
    let ws_server = WsServer::new(api_key.clone(), ws_req_tx, vpn_state_rx, lock_file.clone(), redaction);

    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(4);
    let shutdown_tx_clone = shutdown_tx.clone();
    let vpn_task_cancel_token = vpn_task.cancel_token();
    let server_token = ws_server.cancel_token();

    #[cfg(unix)]
    {
      let vpn_ctx = vpn_task.context();
      let ws_ctx = ws_server.context();

      tokio::spawn(async move { signals::handle_signals(vpn_ctx, ws_ctx).await });
    }

    let vpn_task_handle = tokio::spawn(async move { vpn_task.start(server_token).await });
    let ws_server_handle = tokio::spawn(async move { ws_server.start(shutdown_tx_clone).await });

    #[cfg(debug_assertions)]
    let no_gui = self.no_gui;

    #[cfg(not(debug_assertions))]
    let no_gui = false;

    if no_gui {
      info!("GUI is disabled");
    } else {
      let envs = self.env_file.as_ref().map(env_utils::load_env_vars).transpose()?;

      let minimized = self.minimized;

      tokio::spawn(async move {
        launch_gui(envs, api_key, minimized).await;
        let _ = shutdown_tx.send(()).await;
      });
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
          "[{} {}  {}] {}",
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

  fn prepare_api_key(&self) -> Vec<u8> {
    #[cfg(debug_assertions)]
    if self.no_gui {
      return gpapi::GP_API_KEY.to_vec();
    }

    generate_key().to_vec()
  }
}

#[cfg(unix)]
mod signals {
  use std::sync::Arc;

  use gpapi::utils::runtime;
  use log::{info, warn};

  use crate::vpn_task::VpnTaskContext;
  use crate::ws_server::WsServerContext;

  fn get_disconnected_pid_file() -> anyhow::Result<std::path::PathBuf> {
    runtime::get_disconnected_pid_path()
  }

  pub async fn handle_signals(vpn_ctx: Arc<VpnTaskContext>, ws_ctx: Arc<WsServerContext>) {
    use gpapi::service::event::WsEvent;
    use tokio::signal::unix::{signal, Signal, SignalKind};

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
            match get_disconnected_pid_file() {
              Ok(pid_file_path) => {
                // Ensure the directory exists and is writable
                if let Err(e) = runtime::ensure_lock_file_accessible(&pid_file_path) {
                  warn!("Cannot access disconnected PID file: {}", e);
                } else if let Err(err) = tokio::fs::write(&pid_file_path, pid.to_string()).await {
                  warn!("Failed to write PID to file '{}': {}", pid_file_path.display(), err);
                } else {
                  info!("Wrote disconnected PID to: {}", pid_file_path.display());
                }
              }
              Err(e) => {
                warn!("Failed to determine disconnected PID file path: {}", e);
              }
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

async fn launch_gui(envs: Option<HashMap<String, String>>, api_key: Vec<u8>, mut minimized: bool) {
  loop {
    let gui_launcher = GuiLauncher::new(env!("CARGO_PKG_VERSION"), &api_key)
      .envs(envs.clone())
      .minimized(minimized);

    match gui_launcher.launch().await {
      Ok(exit_status) => {
        // Exit code 99 means that the GUI needs to be restarted
        if exit_status.code() != Some(99) {
          info!("GUI exited with code {:?}", exit_status.code());
          break;
        }

        info!("GUI exited with code 99, restarting");
        minimized = false;
      }
      Err(err) => {
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
