use std::io;

use anyhow::bail;
use clap::{Parser, Subcommand};
use nix::unistd::Uid;

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
    Command::Install => gpservice::vpnc_script::install(io::stdin().lock()),
    Command::Remove => gpservice::vpnc_script::remove(),
  }
}

pub(super) fn run() {
  if let Err(err) = try_run() {
    eprintln!("Error: {err:#}");
    std::process::exit(1);
  }
}
