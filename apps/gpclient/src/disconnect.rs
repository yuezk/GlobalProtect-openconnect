use crate::GP_CLIENT_LOCK_FILE;
use log::{info, warn};
use std::fs;
use sysinfo::{Pid, Signal, System};

pub(crate) struct DisconnectHandler;

impl DisconnectHandler {
  pub(crate) fn new() -> Self {
    Self
  }

  pub(crate) fn handle(&self) -> anyhow::Result<()> {
    if fs::metadata(GP_CLIENT_LOCK_FILE).is_err() {
      warn!("PID file not found, maybe the client is not running");
      return Ok(());
    }

    let pid = fs::read_to_string(GP_CLIENT_LOCK_FILE)?;
    let pid = pid.trim().parse::<usize>()?;
    let s = System::new_all();

    if let Some(process) = s.process(Pid::from(pid)) {
      info!("Found process {}, killing...", pid);
      if process.kill_with(Signal::Interrupt).is_none() {
        warn!("Failed to kill process {}", pid);
      }
    }
    Ok(())
  }
}
