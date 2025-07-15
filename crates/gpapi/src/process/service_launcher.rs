use std::{
  fs::File,
  path::PathBuf,
  process::{ExitStatus, Stdio},
};

use tokio::process::Command;

use crate::GP_SERVICE_BINARY;

use super::command_traits::CommandExt;

pub struct ServiceLauncher<'a> {
  program: PathBuf,
  minimized: bool,
  env_file: Option<String>,
  log_file: Option<String>,
  verbose: Option<&'a str>,
}

impl Default for ServiceLauncher<'_> {
  fn default() -> Self {
    Self::new()
  }
}

impl<'a> ServiceLauncher<'a> {
  pub fn new() -> Self {
    Self {
      program: GP_SERVICE_BINARY.into(),
      minimized: false,
      env_file: None,
      log_file: None,
      verbose: None,
    }
  }

  pub fn minimized(mut self, minimized: bool) -> Self {
    self.minimized = minimized;
    self
  }

  pub fn env_file(mut self, env_file: &str) -> Self {
    self.env_file = Some(env_file.to_string());
    self
  }

  pub fn log_file(mut self, log_file: &str) -> Self {
    self.log_file = Some(log_file.to_string());
    self
  }

  pub fn verbose(mut self, verbose: Option<&'a str>) -> Self {
    self.verbose = verbose;
    self
  }

  pub async fn launch(&self) -> anyhow::Result<ExitStatus> {
    let mut cmd = Command::new_pkexec(&self.program);

    if self.minimized {
      cmd.arg("--minimized");
    }

    if let Some(env_file) = &self.env_file {
      cmd.arg("--env-file").arg(env_file);
    }

    if let Some(verbose) = self.verbose {
      cmd.arg(verbose);
    }

    if let Some(log_file) = &self.log_file {
      let log_file = File::create(log_file)?;
      let stdio = Stdio::from(log_file);
      cmd.stderr(stdio);
    }

    let exit_status = cmd.kill_on_drop(true).spawn()?.wait().await?;

    Ok(exit_status)
  }
}
