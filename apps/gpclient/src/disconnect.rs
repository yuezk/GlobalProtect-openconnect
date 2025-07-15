use clap::Args;
use gpapi::utils::{lock_file::gpservice_lock_info, runtime};
use log::{info, warn};
use std::{fs, str::FromStr, thread, time::Duration};
use sysinfo::{Pid, Signal, System};

#[derive(Args)]
pub struct DisconnectArgs {
  #[arg(
    long,
    required = false,
    help = "The time in seconds to wait for the VPN connection to disconnect"
  )]
  wait: Option<u64>,
}

pub struct DisconnectHandler<'a> {
  args: &'a DisconnectArgs,
}

impl<'a> DisconnectHandler<'a> {
  pub fn new(args: &'a DisconnectArgs) -> Self {
    Self { args }
  }

  pub async fn handle(&self) -> anyhow::Result<()> {
    // Try to disconnect the CLI client
    if let Ok(client_lock_path) = runtime::get_client_lock_path() {
      if let Ok(c) = fs::read_to_string(client_lock_path) {
        send_signal(c.trim(), Signal::Interrupt).unwrap_or_else(|err| {
          warn!("Failed to send signal to client: {}", err);
        });
      }
    } else if let Ok(c) = fs::read_to_string("/var/run/gpclient.lock") {
      send_signal(c.trim(), Signal::Interrupt).unwrap_or_else(|err| {
        warn!("Failed to send signal to client: {}", err);
      });
    }

    // Try to disconnect the GUI service
    if let Ok(c) = gpservice_lock_info().await {
      send_signal(&c.pid.to_string(), Signal::User1).unwrap_or_else(|err| {
        warn!("Failed to send signal to service: {}", err);
      });
    };

    // sleep, to give the client and service time to disconnect
    if let Some(wait) = self.args.wait {
      thread::sleep(Duration::from_secs(wait));
    }

    Ok(())
  }
}

fn send_signal(pid: &str, signal: Signal) -> anyhow::Result<()> {
  let s = System::new_all();
  let pid = Pid::from_str(pid)?;

  if let Some(process) = s.process(pid) {
    info!("Found process {}, sending signal...", pid);

    if process.kill_with(signal).is_none() {
      warn!("Failed to kill process {}", pid);
    }
  }
  Ok(())
}
