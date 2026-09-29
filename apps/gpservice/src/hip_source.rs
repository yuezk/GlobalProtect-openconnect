use std::{
  fs::{self, OpenOptions},
  io::Write,
  os::unix::fs::{OpenOptionsExt, PermissionsExt},
  path::{Path, PathBuf},
  sync::Arc,
};

use anyhow::{Context, bail, ensure};
use gpapi::service::hip::HipSource;
use gpservice::hip_runner_state::{MAX_STATE_BYTES, RUNNER_NAME, RunnerReport, STATE_NAME, validate_root_owned_path};
use tempfile::{Builder, TempDir};

pub(crate) struct HipExecution {
  pub enabled: bool,
  pub uid: u32,
  pub wrapper: Option<String>,
  _state_dir: Option<TempDir>,
  approval: Option<(String, u32)>,
}

impl HipExecution {
  fn disabled() -> Self {
    Self {
      enabled: false,
      uid: 0,
      wrapper: None,
      _state_dir: None,
      approval: None,
    }
  }

  fn script(uid: u32, wrapper: PathBuf, state_dir: Option<TempDir>) -> Self {
    Self {
      enabled: true,
      uid,
      wrapper: Some(wrapper.to_string_lossy().into_owned()),
      _state_dir: state_dir,
      approval: None,
    }
  }

  pub(crate) fn approval_id(&self) -> Option<&str> {
    self.approval.as_ref().map(|(id, _)| id.as_str())
  }

  pub(crate) fn approval_is_valid(&self) -> bool {
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    if let Some((id, uid)) = &self.approval {
      return gpservice::hip_approval::resolve(id, *uid).is_ok();
    }
    true
  }
}

pub(crate) fn resolve(
  source: &HipSource,
  desktop_uid: Option<u32>,
  brokered_macos: bool,
  edited_report: Option<Arc<str>>,
) -> anyhow::Result<HipExecution> {
  match source {
    HipSource::Disabled => Ok(HipExecution::disabled()),
    HipSource::Generated => stage_runner(RunnerReport::Generated),
    HipSource::Edited { .. } => {
      let xml = edited_report.context("Edited HIP report was not uploaded for this connection")?;
      gphip::validate_edited_report(&xml)?;
      stage_runner(RunnerReport::Edited { xml: xml.to_string() })
    }
    HipSource::UserScript { path } => {
      if cfg!(target_os = "macos") || brokered_macos {
        bail!("Custom HIP scripts are unavailable on macOS");
      }
      let uid = desktop_uid.context("HIP custom scripts require a verified desktop user")?;
      ensure!(uid != 0, "HIP custom scripts require a non-root desktop user");
      let path = PathBuf::from(path);
      let metadata = fs::symlink_metadata(&path).context("Failed to inspect the HIP script")?;
      ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.permissions().mode() & 0o111 != 0,
        "HIP custom script must be an executable regular file"
      );
      Ok(HipExecution::script(uid, path, None))
    }
    HipSource::ApprovedRootScript { approval_id } => {
      if cfg!(target_os = "macos") || brokered_macos {
        bail!("Custom HIP scripts are unavailable on macOS");
      }
      #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
      {
        let owner_uid = desktop_uid.context("Root HIP scripts require a verified desktop user")?;
        gpservice::hip_approval::resolve(approval_id, owner_uid)?;
        let mut execution = stage_runner(RunnerReport::ApprovedRootScript {
          approval_id: approval_id.clone(),
          owner_uid,
        })?;
        execution.approval = Some((approval_id.clone(), owner_uid));
        Ok(execution)
      }
      #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
      {
        let _ = (approval_id, desktop_uid);
        bail!("Root HIP script approvals are unavailable on this platform")
      }
    }
  }
}

pub(crate) fn preview_runner(edited_report: Option<Arc<str>>) -> anyhow::Result<HipExecution> {
  let report = if let Some(xml) = edited_report {
    gphip::validate_edited_report(&xml)?;
    RunnerReport::Edited { xml: xml.to_string() }
  } else {
    RunnerReport::Generated
  };
  stage_runner(report)
}

fn stage_runner(report: RunnerReport) -> anyhow::Result<HipExecution> {
  let source = packaged_runner()?;
  let base = runner_state_base();
  fs::create_dir_all(base).context("Cannot create HIP runtime directory")?;
  validate_root_owned_path(base, false)?;
  fs::set_permissions(base, fs::Permissions::from_mode(0o700))?;
  let state_dir = Builder::new().prefix("session-").tempdir_in(base)?;
  fs::set_permissions(state_dir.path(), fs::Permissions::from_mode(0o700))?;
  validate_root_owned_path(state_dir.path(), false)?;

  let runner = state_dir.path().join(RUNNER_NAME);
  fs::copy(&source, &runner).context("Cannot stage HIP runner")?;
  fs::set_permissions(&runner, fs::Permissions::from_mode(0o500))?;
  validate_root_owned_path(&runner, true)?;
  verify_staged_runner(&runner)?;

  let encoded_state = serde_json::to_vec(&report)?;
  ensure!(
    encoded_state.len() <= MAX_STATE_BYTES,
    "HIP runner state exceeds its size limit"
  );
  let state = state_dir.path().join(STATE_NAME);
  let mut state_file = OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(&state)?;
  state_file.write_all(&encoded_state)?;
  state_file.flush()?;
  Ok(HipExecution::script(0, runner, Some(state_dir)))
}

fn runner_state_base() -> &'static Path {
  #[cfg(target_os = "macos")]
  {
    Path::new("/private/var/db/gpclient/hip")
  }
  #[cfg(target_os = "linux")]
  {
    Path::new("/var/lib/gpclient/hip")
  }
  #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
  {
    Path::new("/var/db/gpclient/hip")
  }
}

fn packaged_runner() -> anyhow::Result<PathBuf> {
  #[cfg(target_os = "macos")]
  let path = std::env::current_exe()
    .context("Cannot locate HIP runner")?
    .with_file_name(RUNNER_NAME);
  #[cfg(target_os = "linux")]
  let path = PathBuf::from("/usr/libexec/gpclient/gp-hip-runner");
  #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
  let path = PathBuf::from("/usr/local/libexec/gpclient/gp-hip-runner");

  #[cfg(not(target_os = "macos"))]
  validate_root_owned_path(&path, true)?;
  #[cfg(target_os = "macos")]
  verify_packaged_macos_runner(&path)?;
  ensure!(path.is_file(), "Bundled HIP runner is missing");
  Ok(path)
}

#[cfg(target_os = "macos")]
fn verify_packaged_macos_runner(path: &Path) -> anyhow::Result<()> {
  use std::process::Command;
  let service = std::env::current_exe()?;
  let app = service
    .parent()
    .and_then(Path::parent)
    .and_then(Path::parent)
    .context("gpservice is not in an app bundle")?;
  ensure!(
    path.parent() == service.parent(),
    "HIP runner is outside the app Helpers directory"
  );
  let status = Command::new("/usr/bin/codesign")
    .args(["--verify", "--deep", "--strict"])
    .arg(app)
    .status()?;
  ensure!(status.success(), "HIP application bundle signature is invalid");
  Ok(())
}

#[cfg(target_os = "macos")]
fn verify_staged_runner(path: &Path) -> anyhow::Result<()> {
  use std::process::Command;
  let status = Command::new("/usr/bin/codesign")
    .args(["--verify", "--strict"])
    .arg(path)
    .status()?;
  ensure!(status.success(), "Staged HIP runner signature is invalid");
  let service = std::env::current_exe()?;
  ensure!(
    signing_team(path)? == signing_team(&service)?,
    "HIP runner and service have different signing teams"
  );

  // The staged runner has no app Frameworks directory. Reject any library
  // reference that could resolve through a writable bundle or search path.
  let output = Command::new("/usr/bin/otool").arg("-L").arg(path).output()?;
  ensure!(output.status.success(), "Cannot inspect HIP runner libraries");
  for library in String::from_utf8(output.stdout)?
    .lines()
    .skip(1)
    .filter_map(|line| line.trim().split_whitespace().next())
  {
    ensure!(
      library.starts_with("/usr/lib/") || library.starts_with("/System/Library/"),
      "HIP runner loads a library outside the trusted system paths"
    );
  }
  Ok(())
}

#[cfg(target_os = "macos")]
fn signing_team(path: &Path) -> anyhow::Result<String> {
  let output = std::process::Command::new("/usr/bin/codesign")
    .args(["-d", "--verbose=4"])
    .arg(path)
    .output()?;
  ensure!(output.status.success(), "Cannot inspect code signature");
  let details = String::from_utf8(output.stderr)?;
  details
    .lines()
    .find_map(|line| line.strip_prefix("TeamIdentifier="))
    .filter(|team| !team.is_empty() && *team != "not set")
    .map(str::to_owned)
    .context("Signed HIP binary has no team identifier")
}

#[cfg(not(target_os = "macos"))]
fn verify_staged_runner(_path: &Path) -> anyhow::Result<()> {
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn disabled_has_no_script_on_both_gui_platforms() {
    for brokered_macos in [false, true] {
      let execution = resolve(&HipSource::Disabled, None, brokered_macos, None).unwrap();
      assert!(!execution.enabled);
      assert!(execution.wrapper.is_none());
    }
  }

  #[test]
  fn custom_source_cannot_choose_root_or_run_on_macos() {
    let source = HipSource::UserScript {
      path: "/tmp/hip.sh".into(),
    };
    assert!(resolve(&source, None, false, None).is_err());
    assert!(resolve(&source, Some(0), false, None).is_err());
    assert!(resolve(&source, Some(1000), true, None).is_err());
    #[cfg(target_os = "macos")]
    assert!(resolve(&source, Some(1000), false, None).is_err());
  }

  #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
  #[test]
  fn root_source_requires_a_bound_user_and_installed_approval() {
    let source = HipSource::ApprovedRootScript {
      approval_id: "00000000-0000-4000-8000-000000000000".into(),
    };
    assert!(resolve(&source, None, false, None).is_err());
    assert!(resolve(&source, Some(0), false, None).is_err());
    assert!(resolve(&source, Some(1000), false, None).is_err());
  }
}
