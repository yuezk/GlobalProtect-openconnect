use std::{
  collections::HashMap,
  path::PathBuf,
  process::{ExitStatus, Stdio},
};

use anyhow::bail;
use log::info;
use tokio::{io::AsyncWriteExt, process::Command};

use crate::{process::gui_helper_launcher::GuiHelperLauncher, utils::base64, GP_GUI_BINARY};

use super::command_traits::CommandExt;

pub struct GuiLauncher<'a> {
  version: &'a str,
  program: PathBuf,
  api_key: &'a [u8],
  minimized: bool,
  envs: Option<HashMap<String, String>>,
}

impl<'a> GuiLauncher<'a> {
  pub fn new(version: &'a str, api_key: &'a [u8]) -> Self {
    Self {
      version,
      program: GP_GUI_BINARY.into(),
      api_key,
      minimized: false,
      envs: None,
    }
  }

  pub fn envs<T: Into<Option<HashMap<String, String>>>>(mut self, envs: T) -> Self {
    self.envs = envs.into();
    self
  }

  pub fn minimized(mut self, minimized: bool) -> Self {
    self.minimized = minimized;
    self
  }

  pub async fn launch(&self) -> anyhow::Result<ExitStatus> {
    // Check if the program's version
    if let Err(err) = self.check_version().await {
      info!("Check version failed: {}", err);
      // Download the program and replace the current one
      self.download_program().await?;
    }

    self.launch_program().await
  }

  async fn launch_program(&self) -> anyhow::Result<ExitStatus> {
    let mut cmd = Command::new(&self.program);

    if let Some(envs) = &self.envs {
      cmd.env_clear();
      cmd.envs(envs);
    }

    cmd.arg("--api-key-on-stdin");

    if self.minimized {
      cmd.arg("--minimized");
    }

    info!("Launching gpgui");
    let mut non_root_cmd = cmd.into_non_root()?;
    let mut child = non_root_cmd.kill_on_drop(true).stdin(Stdio::piped()).spawn()?;
    let Some(mut stdin) = child.stdin.take() else {
      bail!("Failed to open stdin");
    };

    let api_key = base64::encode(self.api_key);
    tokio::spawn(async move {
      stdin.write_all(api_key.as_bytes()).await.unwrap();
      drop(stdin);
    });

    let exit_status = child.wait().await?;

    Ok(exit_status)
  }

  async fn check_version(&self) -> anyhow::Result<()> {
    let cmd = Command::new(&self.program).arg("--version").output().await?;
    let output = String::from_utf8_lossy(&cmd.stdout);

    // Version string: "gpgui 2.0.0 (2024-02-05)"
    let Some(version) = output.split_whitespace().nth(1) else {
      bail!("Failed to parse version: {}", output);
    };

    if version != self.version {
      bail!("Version mismatch: expected {}, got {}", self.version, version);
    }

    info!("Version check passed: {}", version);

    Ok(())
  }

  async fn download_program(&self) -> anyhow::Result<()> {
    let gui_helper = GuiHelperLauncher::new(self.api_key);

    gui_helper
      .envs(self.envs.as_ref())
      .gui_version(Some(self.version))
      .launch()
      .await?;

    // Check the version again
    self.check_version().await?;

    Ok(())
  }
}
