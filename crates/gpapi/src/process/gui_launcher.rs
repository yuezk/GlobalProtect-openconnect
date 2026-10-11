use std::{
  collections::HashMap,
  path::{Path, PathBuf},
  process::ExitStatus,
  sync::Arc,
};

use anyhow::bail;
use common::binary_paths;
use log::info;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::{process::gui_helper_launcher::GuiHelperLauncher, service::transport::SessionCredential};

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

  pub(crate) async fn resolve_program(&self, cancellation: &CancellationToken) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
      uzers::get_effective_uid() != 0,
      "The desktop GUI must not be launched by root"
    );
    if let Some(program) = tokio::select! {
      biased;
      _ = cancellation.cancelled() => anyhow::bail!("GUI resolution cancelled"),
      result = self.find_compatible_program() => result,
    } {
      return Ok(program);
    }
    self.download_program(cancellation).await?;
    let program = binary_paths::gpgui_update_target();
    tokio::select! {
      biased;
      _ = cancellation.cancelled() => anyhow::bail!("GUI resolution cancelled"),
      result = self.check_version(&program) => result?,
    }
    Ok(program)
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

  pub(crate) async fn launch_program(
    &self,
    program: &Path,
    cancellation: &CancellationToken,
  ) -> anyhow::Result<ExitStatus> {
    let mut cmd = Command::new(program);
    if let Some(envs) = &self.envs {
      cmd.env_clear().envs(envs);
    }
    cmd.arg("--service-credential-on-stdin");
    if self.minimized {
      cmd.arg("--minimized");
    }
    info!("Launching gpgui");
    super::desktop_child::launch_desktop_child(
      "gpgui",
      cmd.into_std(),
      self.envs.as_ref(),
      &self.credential,
      cancellation,
    )
    .await
  }

  async fn check_version(&self, program: &Path) -> anyhow::Result<()> {
    let mut cmd = Command::new(program);
    if let Some(envs) = &self.envs {
      cmd.env_clear();
      cmd.envs(envs);
    }
    cmd.kill_on_drop(true).arg("--version");
    let output = cmd.output().await?;
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

  async fn download_program(&self, cancellation: &CancellationToken) -> anyhow::Result<()> {
    let gui_helper = GuiHelperLauncher::new(&self.credential);

    gui_helper
      .envs(self.envs.as_ref())
      .gui_version(Some(self.version))
      .launch(cancellation)
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
  async fn captures_gui_logs_in_the_desktop_log_file_across_restarts() {
    let dir = TempDir::new().unwrap();
    let log_file = dir.path().join("desktop.log");
    fs::write(&log_file, "[gpservice] started\n").unwrap();
    let program = dir.path().join("gui");
    let launcher = launcher(VERSION, vec![]).envs(HashMap::from([(
      "GP_LOG_FILE".to_owned(),
      log_file.to_str().unwrap().to_owned(),
    )]));
    let frame_len = launcher.credential.encode_frame().unwrap().len();
    // Consume the credential frame like the real GUI before exiting.
    fs::write(
      &program,
      format!(
        "#!/bin/sh\ndd bs=1 count={frame_len} of=/dev/null 2>/dev/null || exit 1\nprintf '[gpgui] initialized\\n' >&2\n"
      ),
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
      launcher
        .launch_program(&program, &CancellationToken::new())
        .await
        .unwrap()
        .success()
    );
    assert!(
      launcher
        .launch_program(&program, &CancellationToken::new())
        .await
        .unwrap()
        .success()
    );
    assert_eq!(
      fs::read_to_string(log_file).unwrap(),
      "[gpservice] started\n[gpgui] initialized\n[gpgui] initialized\n"
    );
  }

  #[tokio::test]
  async fn cancellation_stops_a_hung_version_probe_before_other_candidates() {
    let dir = TempDir::new().unwrap();
    let pid_file = dir.path().join("version.pid");
    let next_marker = dir.path().join("next-probe");
    let first = dir.path().join("hung");
    let second = dir.path().join("next");
    fs::write(
      &first,
      format!("#!/bin/sh\necho $$ > '{}'; exec sleep 30\n", pid_file.display()),
    )
    .unwrap();
    fs::write(
      &second,
      format!(
        "#!/bin/sh\ntouch '{}'; echo 'gpgui test-version'\n",
        next_marker.display()
      ),
    )
    .unwrap();
    for path in [&first, &second] {
      fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let launcher = launcher(VERSION, vec![first, second]);
    let cancellation = CancellationToken::new();
    let attempt = launcher.resolve_program(&cancellation);
    tokio::pin!(attempt);
    tokio::select! {
      result = &mut attempt => panic!("version probe exited early: {result:?}"),
      ready = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !pid_file.exists() {
          tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
      }) => ready.unwrap(),
    }
    cancellation.cancel();
    assert!(attempt.await.unwrap_err().to_string().contains("cancelled"));
    assert!(!next_marker.exists());
    let pid: i32 = fs::read_to_string(pid_file).unwrap().trim().parse().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
      while nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      }
    })
    .await
    .unwrap();
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
