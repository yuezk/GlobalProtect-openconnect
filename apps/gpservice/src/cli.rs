use std::sync::Arc;
use std::{collections::HashMap, io::Write};

use anyhow::bail;
use clap::Parser;
use gpapi::{
  process::gui_launcher::GuiLauncher,
  service::{request::WsRequest, vpn_state::VpnState},
  utils::{
    crypto::generate_key, env_file, lock_file::LockFile, redact::Redaction, shutdown_signal,
  },
  GP_SERVICE_LOCK_FILE,
};
use log::{info, warn, LevelFilter};
use tokio::sync::{mpsc, watch};

use crate::{vpn_task::VpnTask, ws_server::WsServer};

const VERSION: &str = concat!(
  env!("CARGO_PKG_VERSION"),
  " (",
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
  #[clap(long)]
  no_gui: bool,
}

impl Cli {
  async fn run(&mut self, redaction: Arc<Redaction>) -> anyhow::Result<()> {
    let lock_file = Arc::new(LockFile::new(GP_SERVICE_LOCK_FILE));

    if lock_file.check_health().await {
      bail!("Another instance of the service is already running");
    }

    let api_key = self.prepare_api_key();

    // Channel for sending requests to the VPN task
    let (ws_req_tx, ws_req_rx) = mpsc::channel::<WsRequest>(32);
    // Channel for receiving the VPN state from the VPN task
    let (vpn_state_tx, vpn_state_rx) = watch::channel(VpnState::Disconnected);

    let mut vpn_task = VpnTask::new(ws_req_rx, vpn_state_tx);
    let ws_server = WsServer::new(
      api_key.clone(),
      ws_req_tx,
      vpn_state_rx,
      lock_file.clone(),
      redaction,
    );

    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(4);
    let shutdown_tx_clone = shutdown_tx.clone();
    let vpn_task_token = vpn_task.cancel_token();
    let server_token = ws_server.cancel_token();

    let vpn_task_handle = tokio::spawn(async move { vpn_task.start(server_token).await });
    let ws_server_handle = tokio::spawn(async move { ws_server.start(shutdown_tx_clone).await });

    #[cfg(debug_assertions)]
    let no_gui = self.no_gui;

    #[cfg(not(debug_assertions))]
    let no_gui = false;

    if no_gui {
      info!("GUI is disabled");
    } else {
      let envs = self
        .env_file
        .as_ref()
        .map(env_file::load_env_vars)
        .transpose()?;

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

    vpn_task_token.cancel();
    let _ = tokio::join!(vpn_task_handle, ws_server_handle);

    lock_file.unlock()?;

    info!("gpservice stopped");

    Ok(())
  }

  fn prepare_api_key(&self) -> Vec<u8> {
    #[cfg(debug_assertions)]
    if self.no_gui {
      return gpapi::GP_API_KEY.to_vec();
    }

    generate_key().to_vec()
  }
}

fn init_logger() -> Arc<Redaction> {
  let redaction = Arc::new(Redaction::new());
  let redaction_clone = Arc::clone(&redaction);
  // let target = Box::new(File::create("log.txt").expect("Can't create file"));
  env_logger::builder()
    .filter_level(LevelFilter::Info)
    .format(move |buf, record| {
      let timestamp = buf.timestamp();
      writeln!(
        buf,
        "[{} {} {}] {}",
        timestamp,
        record.level(),
        record.module_path().unwrap_or_default(),
        redaction_clone.redact_str(&record.args().to_string())
      )
    })
    // .target(env_logger::Target::Pipe(target))
    .init();

  redaction
}

async fn launch_gui(envs: Option<HashMap<String, String>>, api_key: Vec<u8>, mut minimized: bool) {
  loop {
    let api_key_clone = api_key.clone();
    let gui_launcher = GuiLauncher::new()
      .envs(envs.clone())
      .api_key(api_key_clone)
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

  let redaction = init_logger();
  info!("gpservice started: {}", VERSION);

  if let Err(e) = cli.run(redaction).await {
    eprintln!("Error: {}", e);
    std::process::exit(1);
  }
}
