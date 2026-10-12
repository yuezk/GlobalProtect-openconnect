use std::{collections::HashMap, path::PathBuf};

use common::binary_paths;
use log::info;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::service::transport::SessionCredential;

pub struct GuiHelperLauncher<'a> {
  program: PathBuf,
  envs: Option<&'a HashMap<String, String>>,
  credential: &'a SessionCredential,
  gui_version: Option<&'a str>,
}

impl<'a> GuiHelperLauncher<'a> {
  pub fn new(credential: &'a SessionCredential) -> Self {
    Self {
      program: binary_paths::gpgui_helper(),
      envs: None,
      credential,
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

  pub async fn launch(&self, cancellation: &CancellationToken) -> anyhow::Result<()> {
    let mut cmd = Command::new(&self.program);

    if let Some(envs) = self.envs {
      cmd.env_clear();
      cmd.envs(envs);
    }

    cmd.arg("--service-credential-on-stdin");

    if let Some(gui_version) = self.gui_version {
      cmd.arg("--gui-version").arg(gui_version);
    }

    info!("Launching gpgui-helper");
    anyhow::ensure!(
      uzers::get_effective_uid() != 0,
      "The desktop GUI must not be launched by root"
    );
    super::desktop_child::launch_desktop_child(
      "gpgui-helper",
      cmd.into_std(),
      self.envs,
      self.credential,
      cancellation,
    )
    .await?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::process::rendering_recovery::RenderingCrash;
  use std::{fs, os::unix::fs::PermissionsExt};

  fn fixture(dir: &tempfile::TempDir, credential: &SessionCredential, body: &str) -> PathBuf {
    let path = dir.path().join("helper");
    let length = credential.encode_frame().unwrap().len();
    fs::write(
      &path,
      format!("#!/bin/sh\ndd bs=1 count={length} of=/dev/null 2>/dev/null || exit 1\n{body}\n"),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
  }

  #[tokio::test]
  async fn propagates_helper_failure_and_preserves_stderr_in_desktop_log() {
    let dir = tempfile::TempDir::new().unwrap();
    let credential = SessionCredential::generate(uuid::Uuid::new_v4(), "test").unwrap();
    let path = fixture(&dir, &credential, "printf 'download failed\\n' >&2; exit 23");
    let log = dir.path().join("desktop.log");
    fs::write(&log, "").unwrap();
    let envs = HashMap::from([("GP_LOG_FILE".into(), log.to_str().unwrap().into())]);
    let launcher = GuiHelperLauncher {
      program: path,
      envs: Some(&envs),
      credential: &credential,
      gui_version: None,
    };
    let error = launcher.launch(&CancellationToken::new()).await.unwrap_err();
    assert!(error.to_string().contains("gpgui-helper exited with"), "{error:#}");
    assert!(error.to_string().contains("23"));
    assert_eq!(fs::read_to_string(log).unwrap(), "download failed\n");
  }

  #[tokio::test]
  async fn recognizes_helper_rendering_crash_and_respects_explicit_override() {
    let dir = tempfile::TempDir::new().unwrap();
    let credential = SessionCredential::generate(uuid::Uuid::new_v4(), "test").unwrap();
    let path = fixture(
      &dir,
      &credential,
      "printf '(gpgui-helper:42): Gdk-Message: 12:00:00: Error 71 (Protocol error) dispatching to Wayland display.\\n' >&2; exit 1",
    );
    let mut envs = HashMap::from([("WAYLAND_DISPLAY".into(), "wayland-0".into())]);
    for override_value in [None, Some(""), Some("0"), Some("1")] {
      if let Some(value) = override_value {
        envs.insert("WEBKIT_DISABLE_DMABUF_RENDERER".into(), value.into());
      }
      let launcher = GuiHelperLauncher {
        program: path.clone(),
        envs: Some(&envs),
        credential: &credential,
        gui_version: None,
      };
      let error = launcher.launch(&CancellationToken::new()).await.unwrap_err();
      let eligible =
        cfg!(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")) && override_value.is_none();
      assert_eq!(error.downcast_ref::<RenderingCrash>().is_some(), eligible, "{error:#}");
    }
  }

  #[tokio::test]
  async fn cancellation_joins_helper_before_returning() {
    let dir = tempfile::TempDir::new().unwrap();
    let credential = SessionCredential::generate(uuid::Uuid::new_v4(), "test").unwrap();
    let pid_file = dir.path().join("pid");
    let body = format!("echo $$ > '{}'; exec sleep 30", pid_file.display());
    let path = fixture(&dir, &credential, &body);
    let envs = HashMap::new();
    let launcher = GuiHelperLauncher {
      program: path,
      envs: Some(&envs),
      credential: &credential,
      gui_version: None,
    };
    let cancellation = CancellationToken::new();
    let attempt = launcher.launch(&cancellation);
    tokio::pin!(attempt);
    tokio::select! {
      result = &mut attempt => panic!("helper exited early: {result:?}"),
      _ = async { while !pid_file.exists() { tokio::time::sleep(std::time::Duration::from_millis(10)).await; } } => {}
    }
    cancellation.cancel();
    assert!(attempt.await.is_err());
    let pid: i32 = fs::read_to_string(pid_file).unwrap().trim().parse().unwrap();
    assert_eq!(
      nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
      Err(nix::errno::Errno::ESRCH)
    );
  }
}
