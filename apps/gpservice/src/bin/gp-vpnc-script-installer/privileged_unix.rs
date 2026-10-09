use std::io::Read;

use anyhow::bail;
use clap::{Parser, Subcommand};
use gpapi::service::vpnc_script::InstallVpncScriptRequest;
use nix::unistd::Uid;

const MAX_INSTALL_REQUEST_SIZE: u64 = 2 * 1024 * 1024;

#[derive(Parser)]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  Install,
  Remove,
}

fn try_run() -> anyhow::Result<()> {
  if !Uid::effective().is_root() {
    bail!("The VPNC script installer must run as root");
  }

  match Cli::parse().command {
    Command::Install => {
      let input = std::io::stdin().lock().take(MAX_INSTALL_REQUEST_SIZE);
      let request = serde_json::from_reader::<_, InstallVpncScriptRequest>(input)?;
      gpservice::vpnc_script::install(request)
    }
    Command::Remove => gpservice::vpnc_script::remove(),
  }
}

pub(super) fn run() {
  if let Err(err) = try_run() {
    eprintln!("Error: {err:#}");
    std::process::exit(1);
  }
}
