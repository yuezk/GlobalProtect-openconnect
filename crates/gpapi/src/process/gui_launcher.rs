use std::{
  collections::HashMap,
  path::PathBuf,
  process::{ExitStatus, Stdio},
};

use tokio::{io::AsyncWriteExt, process::Command};

use crate::{utils::base64, GP_GUI_BINARY};

use super::command_traits::CommandExt;

pub struct GuiLauncher {
  program: PathBuf,
  api_key: Option<Vec<u8>>,
  minimized: bool,
  envs: Option<HashMap<String, String>>,
}

impl Default for GuiLauncher {
  fn default() -> Self {
    Self::new()
  }
}

impl GuiLauncher {
  pub fn new() -> Self {
    Self {
      program: GP_GUI_BINARY.into(),
      api_key: None,
      minimized: false,
      envs: None,
    }
  }

  pub fn envs<T: Into<Option<HashMap<String, String>>>>(mut self, envs: T) -> Self {
    self.envs = envs.into();
    self
  }

  pub fn api_key(mut self, api_key: Vec<u8>) -> Self {
    self.api_key = Some(api_key);
    self
  }

  pub fn minimized(mut self, minimized: bool) -> Self {
    self.minimized = minimized;
    self
  }

  pub async fn launch(&self) -> anyhow::Result<ExitStatus> {
    let mut cmd = Command::new(&self.program);

    if let Some(envs) = &self.envs {
      cmd.env_clear();
      cmd.envs(envs);
    }

    if self.api_key.is_some() {
      cmd.arg("--api-key-on-stdin");
    }

    if self.minimized {
      cmd.arg("--minimized");
    }

    let mut non_root_cmd = cmd.into_non_root()?;

    let mut child = non_root_cmd.kill_on_drop(true).stdin(Stdio::piped()).spawn()?;

    let mut stdin = child
      .stdin
      .take()
      .ok_or_else(|| anyhow::anyhow!("Failed to open stdin"))?;

    if let Some(api_key) = &self.api_key {
      let api_key = base64::encode(api_key);
      tokio::spawn(async move {
        stdin.write_all(api_key.as_bytes()).await.unwrap();
        drop(stdin);
      });
    }

    let exit_status = child.wait().await?;

    Ok(exit_status)
  }
}
