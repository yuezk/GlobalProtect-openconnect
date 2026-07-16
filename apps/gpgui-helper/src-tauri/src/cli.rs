use clap::Parser;
use std::{io::Read, sync::Arc};

use anyhow::{Context, bail};
use gpapi::{
  clap::InfoLevelVerbosity,
  service::transport::{MAX_CREDENTIAL_FRAME, SessionCredential},
  utils::env_utils,
};
use log::info;
use tokio::sync::watch;
use zeroize::Zeroizing;

use crate::app::App;

const VERSION: &str = concat!(
  env!("CARGO_PKG_VERSION"),
  " (",
  env!("GPGUI_HELPER_GIT_COMMIT"),
  " ",
  compile_time::date_str!(),
  ")"
);
#[derive(Parser)]
#[command(version = VERSION)]
struct Cli {
  #[arg(long, help = "Read the service credential from stdin")]
  service_credential_on_stdin: bool,

  #[arg(long, default_value = env!("CARGO_PKG_VERSION"), help = "The version of the GUI")]
  gui_version: String,

  #[command(flatten)]
  verbose: InfoLevelVerbosity,
}

impl Cli {
  fn run(&self) -> anyhow::Result<()> {
    let credential = self.read_credential()?;
    let credential_lease = monitor_credential_lease().context("Failed to monitor service credential lifetime")?;
    let app = App::new(Arc::new(credential), credential_lease, &self.gui_version);

    env_utils::patch_gui_runtime_env(false);

    app.run()
  }

  fn read_credential(&self) -> anyhow::Result<SessionCredential> {
    if !self.service_credential_on_stdin {
      bail!("Service credential must be provided via --service-credential-on-stdin");
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut header = [0_u8; 2];
    input
      .read_exact(&mut header)
      .context("Failed to read service credential header")?;
    let payload_len = u16::from_be_bytes(header) as usize;
    if payload_len == 0 || payload_len + header.len() > MAX_CREDENTIAL_FRAME {
      bail!("Invalid service credential length");
    }
    let mut frame = Zeroizing::new(Vec::with_capacity(payload_len + header.len()));
    frame.extend_from_slice(&header);
    frame.resize(payload_len + header.len(), 0);
    input
      .read_exact(&mut frame[header.len()..])
      .context("Failed to read service credential")?;
    SessionCredential::decode_frame(&frame).context("Invalid service credential")
  }
}

fn monitor_credential_lease() -> std::io::Result<watch::Receiver<bool>> {
  let (lease_tx, lease_rx) = watch::channel(true);
  std::thread::Builder::new()
    .name("credential-lease".to_owned())
    .spawn(move || {
      let stdin = std::io::stdin();
      close_credential_lease_at_eof(stdin.lock(), lease_tx);
    })?;
  Ok(lease_rx)
}

fn close_credential_lease_at_eof(mut input: impl Read, lease_tx: watch::Sender<bool>) {
  let mut buffer = [0_u8; 256];
  while matches!(input.read(&mut buffer), Ok(len) if len > 0) {}
  let _ = lease_tx.send(false);
}

fn init_logger(cli: &Cli) {
  env_logger::builder()
    .filter_level(cli.verbose.log_level_filter())
    .init();
}

pub fn run() {
  let cli = Cli::parse();

  init_logger(&cli);
  info!("gpgui-helper started: {}", VERSION);

  if let Err(e) = cli.run() {
    eprintln!("{}", e);
    std::process::exit(1);
  }
}

#[cfg(test)]
mod tests {
  use std::io::Cursor;

  use super::*;

  #[test]
  fn credential_lease_closes_at_eof() {
    let (lease_tx, lease_rx) = watch::channel(true);
    close_credential_lease_at_eof(Cursor::new(b"ignored lease bytes"), lease_tx);
    assert!(!*lease_rx.borrow());
  }
}
