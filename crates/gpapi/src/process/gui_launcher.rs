use std::{
  collections::HashMap,
  path::{Path, PathBuf},
  process::ExitStatus,
  sync::Arc,
};

use anyhow::bail;
use log::info;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::{process::gui_helper_launcher::GuiHelperLauncher, service::transport::SessionCredential};

pub struct GuiLauncher<'a> {
  version: &'a str,
  variant: crate::gui::GuiVariant,
  programs: Vec<PathBuf>,
  credential: Arc<SessionCredential>,
  minimized: bool,
  envs: Option<HashMap<String, String>>,
}

impl<'a> GuiLauncher<'a> {
  pub fn new(version: &'a str, credential: Arc<SessionCredential>, variant: crate::gui::GuiVariant) -> Self {
    let primary = variant.primary();
    let update_target = variant.update_target();
    let programs = if primary == update_target {
      vec![primary]
    } else {
      vec![primary, update_target]
    };

    Self {
      version,
      variant,
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
    if let Some(program) = self.find_compatible_program(cancellation).await? {
      return Ok(program);
    }
    self.download_program(cancellation).await?;
    let program = self.variant.update_target();
    tokio::select! {
      biased;
      _ = cancellation.cancelled() => anyhow::bail!("GUI resolution cancelled"),
      result = self.check_version(&program) => result?,
    }
    Ok(program)
  }

  async fn find_compatible_program(&self, cancellation: &CancellationToken) -> anyhow::Result<Option<PathBuf>> {
    for program in &self.programs {
      let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => anyhow::bail!("GUI resolution cancelled"),
        result = self.check_version(program) => result,
      };
      match result {
        Ok(()) => return Ok(Some(program.clone())),
        Err(err) => info!(
          "GUI candidate {} is unavailable or incompatible: {err}",
          program.display()
        ),
      }
    }

    Ok(None)
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
    info!("Launching {}", program.display());
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
      .variant(self.variant)
      .launch(cancellation)
      .await
  }

  #[cfg(target_os = "linux")]
  pub(crate) async fn launch_with_switches(
    &self,
    program: &Path,
    cancellation: &CancellationToken,
    listener: &tokio::net::UnixListener,
    service_input: &mut (impl tokio::io::AsyncWrite + Unpin),
    service_output: &mut (impl tokio::io::AsyncRead + Unpin),
  ) -> anyhow::Result<ExitStatus> {
    use super::gui_switch;
    use crate::gui::GuiSwitchOutcome;
    use crate::service::transport::{DESKTOP_SWITCH_BEGIN, DESKTOP_SWITCH_CANCEL, DESKTOP_SWITCH_COMMIT};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut current = Some(self.start_owned(program.to_owned(), cancellation.child_token()));
    let mut retired = Vec::new();
    let mut unexpected = [0];
    let result = async { loop {
      tokio::select! {
        biased;
        outcome = &mut current.as_mut().unwrap().task => {
          current.take();
          break outcome?;
        }
        accepted = listener.accept() => {
          let (mut socket, _) = accepted?;
          let mut pending = false;
          let replacement = async {
            let selection = gui_switch::receive(&mut socket).await?;
            let variant = selection.variant;
            let candidate = Self::new(self.version, self.credential.clone(), variant).envs(self.envs.clone());
            let program = candidate.find_compatible_program(cancellation).await?;
            if program.is_none() && !selection.allow_download {
              gui_switch::reply(&mut socket, Ok(GuiSwitchOutcome::DownloadRequired)).await?;
              return Ok(None);
            }
            service_input.write_all(&[DESKTOP_SWITCH_BEGIN]).await?;
            pending = true;
            let credential = super::desktop_launcher::read_credential(service_output).await?
              .ok_or_else(|| anyhow::anyhow!("Service closed before issuing the switch credential"))?;
            anyhow::ensure!(credential.product_version() == self.version, "Service product version mismatch");
            let ready = gui_switch::SwitchListener::new()?;
            let mut envs = self.envs.clone().unwrap_or_else(|| std::env::vars().collect());
            envs.insert(gui_switch::READY_ENV.into(), ready.path().to_string_lossy().into_owned());
            let launcher = Self::new(self.version, Arc::new(credential), variant)
              .envs(envs)
              .minimized(false);
            let program = match program {
              Some(program) => program,
              None => launcher.resolve_program(cancellation).await?,
            };
            let mut child = launcher.start_owned(program, cancellation.child_token());
            let start_result = tokio::select! {
              result = tokio::time::timeout(std::time::Duration::from_secs(30), ready.ready()) => {
                result.map_err(|_| anyhow::anyhow!("The selected GUI did not finish starting" )).and_then(|result| result)
              },
              outcome = &mut child.task => {
                return match outcome? {
                  Ok(status) => Err(anyhow::anyhow!("The selected GUI exited during startup: {status}")),
                  Err(error) => Err(error),
                };
              }
            };
            if let Err(error) = start_result.and_then(|()| crate::gui::save_preference(variant)) {
              child.cancel.cancel();
              let _ = child.task.await;
              return Err(error);
            }
            if let Err(error) = gui_switch::reply(&mut socket, Ok(GuiSwitchOutcome::Switched)).await {
              child.cancel.cancel();
              let _ = child.task.await;
              return Err(error);
            }
            if let Err(error) = service_input.write_all(&[DESKTOP_SWITCH_COMMIT]).await {
              child.cancel.cancel();
              let _ = child.task.await;
              return Err(error.into());
            }
            pending = false;
            Ok(Some(child))
          }.await;
          match replacement {
            Ok(Some(child)) => retired.push(current.replace(child).unwrap()),
            Ok(None) => {},
            Err(error) => {
              if pending {
                service_input.write_all(&[DESKTOP_SWITCH_CANCEL]).await?;
              }
              let _ = gui_switch::reply(&mut socket, Err(format!("{error:#}"))).await;
            }
          }
        }
        result = service_output.read(&mut unexpected) => {
          break match result {
            Ok(0) => Err(anyhow::anyhow!("The privileged service closed its credential channel")),
            Ok(_) => Err(anyhow::anyhow!("Unexpected service credential data while the GUI is running")),
            Err(error) => Err(error.into()),
          };
        }
      }
    }}.await;
    // Every GUI remains owned by this supervisor until it is reaped. The retiring
    // GUI exits after the successful switch reply; cancellation also joins it.
    if let Some(child) = current {
      retired.push(child);
    }
    for child in &retired {
      child.cancel.cancel();
    }
    for child in retired {
      let _ = child.task.await;
    }
    result
  }

  #[cfg(target_os = "linux")]
  fn start_owned(&self, program: PathBuf, cancel: CancellationToken) -> RunningGui {
    let version = self.version.to_owned();
    let variant = self.variant;
    let credential = self.credential.clone();
    let envs = self.envs.clone();
    let minimized = self.minimized;
    let task_cancel = cancel.clone();
    let task = tokio::spawn(async move {
      GuiLauncher::new(&version, credential, variant)
        .envs(envs)
        .minimized(minimized)
        .launch_program(&program, &task_cancel)
        .await
    });
    RunningGui { task, cancel }
  }
}

#[cfg(target_os = "linux")]
struct RunningGui {
  task: tokio::task::JoinHandle<anyhow::Result<ExitStatus>>,
  cancel: CancellationToken,
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
      variant: crate::gui::GuiVariant::Tauri,
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

    assert_eq!(
      launcher
        .find_compatible_program(&CancellationToken::new())
        .await
        .unwrap(),
      Some(packaged)
    );
  }

  #[tokio::test]
  async fn falls_back_when_the_first_program_is_incompatible() {
    let dir = TempDir::new().unwrap();
    let packaged = executable(&dir, "packaged", "gpgui other-version", 0);
    let downloaded = executable(&dir, "downloaded", "gpgui test-version", 0);
    let launcher = launcher(VERSION, vec![packaged, downloaded.clone()]);

    assert_eq!(
      launcher
        .find_compatible_program(&CancellationToken::new())
        .await
        .unwrap(),
      Some(downloaded)
    );
  }

  #[tokio::test]
  async fn rejects_an_unsuccessful_version_command() {
    let dir = TempDir::new().unwrap();
    let program = executable(&dir, "broken", "gpgui test-version", 1);
    let launcher = launcher(VERSION, vec![program]);

    assert_eq!(
      launcher
        .find_compatible_program(&CancellationToken::new())
        .await
        .unwrap(),
      None
    );
  }

  #[tokio::test]
  async fn nsh_version_mismatch_cannot_select_a_compatible_tauri_binary() {
    let dir = TempDir::new().unwrap();
    let _tauri = executable(&dir, "gpgui", "gpgui test-version", 0);
    let nsh = executable(&dir, "gpgui-nsh", "gpgui-nsh old-version", 0);
    let mut selected = launcher(VERSION, vec![nsh.clone()]);
    selected.variant = crate::gui::GuiVariant::Nsh;
    assert!(
      selected
        .find_compatible_program(&CancellationToken::new())
        .await
        .unwrap()
        .is_none()
    );
    executable(&dir, "gpgui-nsh", "gpgui-nsh test-version", 0);
    assert_eq!(
      selected
        .find_compatible_program(&CancellationToken::new())
        .await
        .unwrap(),
      Some(nsh)
    );
  }

  #[test]
  #[cfg(target_os = "linux")]
  fn supervisor_switch_success_and_failure_preserve_child_ownership() {
    for scenario in ["success", "failure", "missing", "incompatible"] {
      let directory = TempDir::new().unwrap();
      let executable = std::env::current_exe().unwrap();
      for name in ["gpgui", "gpgui-nsh"] {
        if scenario == "missing" && name == "gpgui-nsh" {
          continue;
        }
        let version = if scenario == "incompatible" && name == "gpgui-nsh" {
          "old-version"
        } else {
          VERSION
        };
        let path = directory.path().join(name);
        fs::write(&path, format!(
          "#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{name} {version}'; exit 0; fi\nGP_SWITCH_GUI_VARIANT={name} exec '{}' --exact process::gui_launcher::tests::gui_process_fixture --ignored --nocapture\n",
          executable.display()
        )).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
      }
      let output = std::process::Command::new(&executable)
        .args([
          "--exact",
          "process::gui_launcher::tests::supervisor_fixture",
          "--ignored",
          "--nocapture",
        ])
        .env("GP_SWITCH_SCENARIO", scenario)
        .env("GP_SWITCH_DIRECTORY", directory.path())
        .env("GP_GUI_BINARY", directory.path().join("gpgui"))
        .env("XDG_CONFIG_HOME", directory.path().join("config"))
        .output()
        .unwrap();
      assert!(
        output.status.success(),
        "{scenario}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
      );
    }
  }

  #[test]
  #[cfg(target_os = "linux")]
  fn selected_nsh_upgrade_downloads_nsh_without_probing_tauri() {
    let directory = TempDir::new().unwrap();
    let test_executable = std::env::current_exe().unwrap();
    let tauri_marker = directory.path().join("tauri-probed");
    let tauri = executable(&directory, "gpgui", "gpgui test-version", 0);
    fs::write(
      &tauri,
      format!(
        "#!/bin/sh\ntouch '{}'; echo 'gpgui {VERSION}'\n",
        tauri_marker.display()
      ),
    )
    .unwrap();
    executable(&directory, "gpgui-nsh", "gpgui-nsh old-version", 0);
    let helper = directory.path().join("gpgui-helper");
    fs::write(&helper, format!(
      "#!/bin/sh\n[ \"$2\" = --gui-variant ] && [ \"$3\" = nsh ] || exit 21\nexec '{}' --exact process::gui_launcher::tests::upgrade_helper_fixture --ignored --nocapture\n",
      test_executable.display()
    )).unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let result = std::process::Command::new(test_executable)
      .args([
        "--exact",
        "process::gui_launcher::tests::upgrade_fixture",
        "--ignored",
        "--nocapture",
      ])
      .env("GP_SWITCH_DIRECTORY", directory.path())
      .env("GP_GUI_BINARY", &tauri)
      .env("GP_GUI_HELPER_BINARY", helper)
      .env("XDG_CONFIG_HOME", directory.path().join("config"))
      .output()
      .unwrap();
    assert!(
      result.status.success(),
      "{} {}",
      String::from_utf8_lossy(&result.stdout),
      String::from_utf8_lossy(&result.stderr)
    );
    assert!(!tauri_marker.exists());
    assert!(directory.path().join("nsh-updated").exists());
  }

  #[tokio::test]
  #[ignore = "subprocess upgrade resolver fixture"]
  #[cfg(target_os = "linux")]
  async fn upgrade_fixture() {
    crate::gui::save_preference(crate::gui::GuiVariant::Nsh).unwrap();
    let credential = Arc::new(SessionCredential::generate(Uuid::new_v4(), VERSION).unwrap());
    let selected = GuiLauncher::new(VERSION, credential, crate::gui::preferred().unwrap());
    let program = selected.resolve_program(&CancellationToken::new()).await.unwrap();
    assert_eq!(program.file_name().unwrap(), "gpgui-nsh");
  }

  #[test]
  #[ignore = "subprocess download helper fixture"]
  #[cfg(target_os = "linux")]
  fn upgrade_helper_fixture() {
    use std::io::Read;
    let mut input = std::io::stdin().lock();
    let mut header = [0; 2];
    input.read_exact(&mut header).unwrap();
    let mut frame = zeroize::Zeroizing::new(vec![0; usize::from(u16::from_be_bytes(header))]);
    input.read_exact(&mut frame).unwrap();
    let directory = PathBuf::from(std::env::var_os("GP_SWITCH_DIRECTORY").unwrap());
    fs::write(
      directory.join("gpgui-nsh"),
      format!("#!/bin/sh\necho 'gpgui-nsh {VERSION}'\n"),
    )
    .unwrap();
    fs::write(directory.join("nsh-updated"), "").unwrap();
  }

  #[test]
  #[ignore = "subprocess GUI fixture"]
  #[cfg(target_os = "linux")]
  fn gui_process_fixture() {
    use std::io::Read;
    let mut input = std::io::stdin().lock();
    let mut header = [0; 2];
    input.read_exact(&mut header).unwrap();
    let mut frame = zeroize::Zeroizing::new(vec![0; usize::from(u16::from_be_bytes(header))]);
    input.read_exact(&mut frame).unwrap();
    let directory = PathBuf::from(std::env::var_os("GP_SWITCH_DIRECTORY").unwrap());
    let variant = std::env::var("GP_SWITCH_GUI_VARIANT").unwrap();
    if variant == "gpgui-nsh" && std::env::var("GP_SWITCH_SCENARIO").unwrap() == "failure" {
      panic!("fixture: native host failed to initialize");
    }
    super::super::gui_switch::notify_ready().unwrap();
    fs::write(directory.join(format!("{variant}-started")), "").unwrap();
    while !directory.join(format!("stop-{variant}")).exists() {
      std::thread::sleep(std::time::Duration::from_millis(10));
    }
    fs::write(directory.join(format!("{variant}-exited")), "").unwrap();
  }

  #[tokio::test]
  #[ignore = "subprocess supervisor fixture with isolated GUI paths and preferences"]
  #[cfg(target_os = "linux")]
  async fn supervisor_fixture() {
    use crate::service::transport::{DESKTOP_SWITCH_BEGIN, DESKTOP_SWITCH_CANCEL, DESKTOP_SWITCH_COMMIT};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let directory = PathBuf::from(std::env::var_os("GP_SWITCH_DIRECTORY").unwrap());
    let scenario = std::env::var("GP_SWITCH_SCENARIO").unwrap();
    let success = scenario == "success";
    let download_required = matches!(scenario.as_str(), "missing" | "incompatible");
    let listener = super::super::gui_switch::SwitchListener::new().unwrap();
    let (mut service_input, mut control) = tokio::io::duplex(64);
    let (mut service_output, mut credentials) = tokio::io::duplex(2048);
    let service_marker = directory.join("service-switch-started");
    let service = tokio::spawn(async move {
      loop {
        let message = control.read_u8().await.unwrap();
        match message {
          DESKTOP_SWITCH_BEGIN => {
            fs::write(&service_marker, "").unwrap();
            let credential = SessionCredential::generate(Uuid::new_v4(), VERSION).unwrap();
            credentials
              .write_all(&credential.encode_frame().unwrap())
              .await
              .unwrap();
          }
          DESKTOP_SWITCH_COMMIT | DESKTOP_SWITCH_CANCEL => {}
          _ => panic!("unexpected fixture desktop control"),
        }
      }
    });
    let envs = std::env::vars().collect::<HashMap<_, _>>();
    let launcher = launcher(VERSION, vec![]).envs(envs);
    let cancel = CancellationToken::new();
    let program = directory.join("gpgui");
    let run = launcher.launch_with_switches(
      &program,
      &cancel,
      &listener.listener,
      &mut service_input,
      &mut service_output,
    );
    tokio::pin!(run);
    let checkpoint = async {
      while !directory.join("gpgui-started").exists() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      }
      let result = super::super::gui_switch::request_at(
        &listener.path(),
        crate::gui::GuiSwitchRequest {
          variant: crate::gui::GuiVariant::Nsh,
          allow_download: false,
        },
      )
      .await;
      if download_required {
        anyhow::ensure!(
          result? == crate::gui::GuiSwitchOutcome::DownloadRequired,
          "Missing download confirmation"
        );
        anyhow::ensure!(
          !directory.join("service-switch-started").exists(),
          "Download check changed the service session"
        );
        anyhow::ensure!(
          !directory.join("gpgui-nsh-started").exists(),
          "Download check launched the replacement"
        );
      } else {
        anyhow::ensure!(result.is_ok() == success, "Unexpected switch result: {result:?}");
        if success {
          anyhow::ensure!(
            result? == crate::gui::GuiSwitchOutcome::Switched,
            "Compatible GUI did not switch directly"
          );
        }
      }
      anyhow::ensure!(
        !directory.join("gpgui-exited").exists(),
        "Current GUI exited before acknowledgement"
      );
      if success {
        anyhow::ensure!(
          crate::gui::preferred()? == crate::gui::GuiVariant::Nsh,
          "Successful switch did not save preference"
        );
        fs::write(directory.join("stop-gpgui"), "")?;
        while !directory.join("gpgui-exited").exists() {
          tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
      } else {
        anyhow::ensure!(
          crate::gui::preferred()? == crate::gui::GuiVariant::Tauri,
          "Failed switch changed preference"
        );
      }
      Ok::<_, anyhow::Error>(())
    };
    let checkpoint_result = tokio::select! {
      result = &mut run => panic!("supervisor exited before the owned GUI: {result:?}"),
      result = tokio::time::timeout(std::time::Duration::from_secs(10), checkpoint) => result.map_err(anyhow::Error::from).and_then(|result| result),
    };
    if let Err(error) = checkpoint_result {
      cancel.cancel();
      let _ = run.await;
      panic!("{error:#}");
    }
    let stop = if success { "stop-gpgui-nsh" } else { "stop-gpgui" };
    fs::write(directory.join(stop), "").unwrap();
    assert!(
      tokio::time::timeout(std::time::Duration::from_secs(5), run)
        .await
        .unwrap()
        .unwrap()
        .success()
    );
    service.abort();
  }
}
