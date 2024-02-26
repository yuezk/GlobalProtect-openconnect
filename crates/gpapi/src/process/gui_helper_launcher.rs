use std::{collections::HashMap, path::PathBuf, process::Stdio};

use anyhow::bail;
use log::info;
use tokio::{io::AsyncWriteExt, process::Command};

use crate::{process::command_traits::CommandExt, utils, GP_GUI_HELPER_BINARY};

pub struct GuiHelperLauncher<'a> {
  program: PathBuf,
  envs: Option<&'a HashMap<String, String>>,
  api_key: &'a [u8],
  gui_version: Option<&'a str>,
}

impl<'a> GuiHelperLauncher<'a> {
  pub fn new(api_key: &'a [u8]) -> Self {
    Self {
      program: GP_GUI_HELPER_BINARY.into(),
      envs: None,
      api_key,
      gui_version: None,
    }
  }

  pub fn envs(mut self, envs: Option<&'a HashMap<String, String>>) -> Self {
    self.envs = envs;
    self
  }

  pub fn gui_version(mut self, version: Option<&'a str>) -> Self {
    self.gui_version = version;
    self
  }

  pub async fn launch(&self) -> anyhow::Result<()> {
    let mut cmd = Command::new(&self.program);

    if let Some(envs) = self.envs {
      cmd.env_clear();
      cmd.envs(envs);
    }

    cmd.arg("--api-key-on-stdin");

    if let Some(gui_version) = self.gui_version {
      cmd.arg("--gui-version").arg(gui_version);
    }

    info!("Launching gpgui-helper");
    let mut non_root_cmd = cmd.into_non_root()?;
    let mut child = non_root_cmd.kill_on_drop(true).stdin(Stdio::piped()).spawn()?;
    let Some(mut stdin) = child.stdin.take() else {
      bail!("Failed to open stdin");
    };

    let api_key = utils::base64::encode(self.api_key);
    tokio::spawn(async move {
      stdin.write_all(api_key.as_bytes()).await.unwrap();
      drop(stdin);
    });

    let exit_status = child.wait().await?;
    info!("gpgui-helper exited with: {}", exit_status);

    Ok(())
  }
}
