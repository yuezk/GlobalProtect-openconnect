use std::{
  collections::HashMap,
  path::{Path, PathBuf},
  process::{ExitStatus, Stdio},
  sync::Arc,
};

use anyhow::bail;
use common::binary_paths;
use log::info;
use tokio::{io::AsyncWriteExt, process::Command};

use crate::{process::gui_helper_launcher::GuiHelperLauncher, service::transport::SessionCredential};

use super::command_traits::CommandExt;

pub struct GuiLauncher<'a> {
  version: &'a str,
  programs: Vec<PathBuf>,
  credential: Arc<SessionCredential>,
  minimized: bool,
  envs: Option<HashMap<String, String>>,
}

impl<'a> GuiLauncher<'a> {
  pub fn new(version: &'a str, credential: Arc<SessionCredential>) -> Self {
    let primary = binary_paths::gpgui();
    let update_target = binary_paths::gpgui_update_target();
    let programs = if primary == update_target {
      vec![primary]
    } else {
      vec![primary, update_target]
    };

    Self {
      version,
      programs,
      credential,
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
    if let Some(program) = self.find_compatible_program().await {
      return self.launch_program(&program).await;
    }

    self.download_program().await?;

    let program = binary_paths::gpgui_update_target();
    self.check_version(&program).await?;
    self.launch_program(&program).await
  }

  async fn find_compatible_program(&self) -> Option<PathBuf> {
    for program in &self.programs {
      match self.check_version(program).await {
        Ok(()) => return Some(program.clone()),
        Err(err) => info!(
          "GUI candidate {} is unavailable or incompatible: {err}",
          program.display()
        ),
      }
    }

    None
  }

  async fn launch_program(&self, program: &Path) -> anyhow::Result<ExitStatus> {
    let mut cmd = Command::new(program);

    if let Some(envs) = &self.envs {
      cmd.env_clear();
      cmd.envs(envs);
    }

    cmd.arg("--service-credential-on-stdin");

    if self.minimized {
      cmd.arg("--minimized");
    }

    info!("Launching gpgui");
    let mut non_root_cmd = cmd.into_non_root()?;
    let child = non_root_cmd.kill_on_drop(true).stdin(Stdio::piped()).spawn();
    let mut child = match child {
      Ok(child) => child,
      Err(err) => bail!("Failed to spawn {}: {}", program.display(), err),
    };

    let Some(mut stdin) = child.stdin.take() else {
      bail!("Failed to open stdin");
    };

    let frame = self.credential.encode_frame()?;
    stdin.write_all(&frame).await?;

    let exit_status = child.wait().await?;
    drop(stdin);

    Ok(exit_status)
  }

  async fn check_version(&self, program: &Path) -> anyhow::Result<()> {
    let mut cmd = Command::new(program);
    if let Some(envs) = &self.envs {
      cmd.env_clear();
      cmd.envs(envs);
    }
    cmd.arg("--version");
    let output = cmd.into_non_root()?.output().await?;
    if !output.status.success() {
      bail!("Version command exited with {}", output.status);
    }
    let output = String::from_utf8_lossy(&output.stdout);

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
    let gui_helper = GuiHelperLauncher::new(&self.credential);

    gui_helper
      .envs(self.envs.as_ref())
      .gui_version(Some(self.version))
      .launch()
      .await
  }
}

#[cfg(test)]
mod tests {
  use std::{fs, os::unix::fs::PermissionsExt};

  use tempfile::TempDir;
  use uuid::Uuid;

  use super::*;

  const VERSION: &str = "test-version";

  fn executable(dir: &TempDir, name: &str, output: &str, exit_code: i32) -> PathBuf {
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let path = dir.path().join(name);
    fs::write(
      &path,
      format!("#!/bin/sh\nprintf '%s\\n' '{output}'\nexit {exit_code}\n"),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
  }

  fn launcher(version: &'static str, programs: Vec<PathBuf>) -> GuiLauncher<'static> {
    GuiLauncher {
      version,
      programs,
      credential: Arc::new(SessionCredential::generate(Uuid::new_v4(), version).unwrap()),
      minimized: false,
      envs: None,
    }
  }

  #[tokio::test]
  async fn prefers_the_first_compatible_program() {
    let dir = TempDir::new().unwrap();
    let packaged = executable(&dir, "packaged", "gpgui test-version", 0);
    let downloaded = executable(&dir, "downloaded", "gpgui test-version", 0);
    let launcher = launcher(VERSION, vec![packaged.clone(), downloaded]);

    assert_eq!(launcher.find_compatible_program().await, Some(packaged));
  }

  #[tokio::test]
  async fn falls_back_when_the_first_program_is_incompatible() {
    let dir = TempDir::new().unwrap();
    let packaged = executable(&dir, "packaged", "gpgui other-version", 0);
    let downloaded = executable(&dir, "downloaded", "gpgui test-version", 0);
    let launcher = launcher(VERSION, vec![packaged, downloaded.clone()]);

    assert_eq!(launcher.find_compatible_program().await, Some(downloaded));
  }

  #[tokio::test]
  async fn rejects_an_unsuccessful_version_command() {
    let dir = TempDir::new().unwrap();
    let program = executable(&dir, "broken", "gpgui test-version", 1);
    let launcher = launcher(VERSION, vec![program]);

    assert_eq!(launcher.find_compatible_program().await, None);
  }
}
