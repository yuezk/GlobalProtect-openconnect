use std::io::Read;

use anyhow::{Context, ensure};
use clap::{Parser, Subcommand};
use gpservice::hip_approval::{self, InstallRequest};
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
  Revoke { approval_id: String },
}

fn try_run() -> anyhow::Result<()> {
  ensure!(Uid::effective().is_root(), "HIP script installer must run as root");
  let approving_uid = std::env::var("PKEXEC_UID")
    .context("HIP script installer requires pkexec authorization")?
    .parse::<u32>()
    .context("Invalid pkexec user identity")?;
  ensure!(approving_uid != 0, "HIP script installer requires a desktop user");

  match Cli::parse().command {
    Command::Install => {
      let mut input = Vec::new();
      std::io::stdin()
        .lock()
        .take(MAX_INSTALL_REQUEST_SIZE + 1)
        .read_to_end(&mut input)?;
      ensure!(
        input.len() as u64 <= MAX_INSTALL_REQUEST_SIZE,
        "HIP install request is too large"
      );
      let request: InstallRequest = serde_json::from_slice(&input).context("Invalid HIP install request")?;
      let info = hip_approval::install(request, approving_uid)?;
      serde_json::to_writer(std::io::stdout().lock(), &info)?;
      println!();
    }
    Command::Revoke { approval_id } => hip_approval::revoke(&approval_id, approving_uid)?,
  }
  Ok(())
}

pub(super) fn run() {
  if let Err(err) = try_run() {
    eprintln!("HIP approval failed: {err:#}");
    std::process::exit(1);
  }
}
