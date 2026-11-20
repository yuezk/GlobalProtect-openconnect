use super::{
  command_runner::{ChildInput, run_observed},
  rendering_recovery::{
    GbmBufferFailure, RenderingCrash, StderrDiagnostic, allows_shared_memory_override, eligible_with_inheritance,
    is_crash_exit,
  },
};
use crate::{gui::GuiVariant, service::transport::SessionCredential};
use anyhow::Context;
use std::{
  cell::Cell,
  collections::HashMap,
  fs::OpenOptions,
  path::PathBuf,
  process::{Command, ExitStatus},
};
use tokio_util::sync::CancellationToken;

/// Monitor a credential-bearing desktop child; the owner cancels and joins this attempt.
pub(super) async fn launch_desktop_child(
  name: &'static str,
  command: Command,
  envs: Option<&HashMap<String, String>>,
  credential: &SessionCredential,
  cancellation: &CancellationToken,
  interface: Option<GuiVariant>,
) -> anyhow::Result<ExitStatus> {
  let retry_eligible = interface != Some(GuiVariant::Nsh) && eligible_with_inheritance(&command, envs.is_none());
  let detect_gbm_failure =
    interface == Some(GuiVariant::Tauri) && allows_shared_memory_override(&command, envs.is_none());
  let log_file = match envs {
    Some(envs) => envs.get("GP_LOG_FILE").map(PathBuf::from),
    None => std::env::var_os("GP_LOG_FILE").map(PathBuf::from),
  };
  let mut sink: Box<dyn std::io::Write + Send> = match log_file {
    Some(path) => Box::new(
      OpenOptions::new()
        .append(true)
        .open(&path)
        .with_context(|| format!("Failed to open GUI log file: {}", path.display()))?,
    ),
    None => Box::new(std::io::stderr()),
  };
  let frame = credential.encode_frame()?;
  let cancellation = cancellation.clone();

  tokio::task::spawn_blocking(move || {
    let gbm_failed = Cell::new(false);
    let check = || {
      if cancellation.is_cancelled() {
        Err(std::io::Error::new(
          std::io::ErrorKind::Interrupted,
          "Desktop child launch cancelled",
        ))
      } else if gbm_failed.get() {
        Err(std::io::Error::other(GbmBufferFailure))
      } else {
        Ok(())
      }
    };
    let mut diagnostic = StderrDiagnostic::default();
    let observed = run_observed(
      command,
      &check,
      None,
      &mut |bytes| {
        diagnostic.push(bytes);
        gbm_failed.set(detect_gbm_failure && diagnostic.should_retry_gbm(retry_eligible));
      },
      &mut sink,
      ChildInput::Frame(&frame),
    );
    if gbm_failed.get() && !cancellation.is_cancelled() {
      return Err(GbmBufferFailure.into());
    }
    let observed = observed?;
    let output = observed.output;
    if is_crash_exit(output.status) && retry_eligible && diagnostic.matched() {
      return Err(RenderingCrash { status: output.status }.into());
    }
    if let Some(error) = observed.input_error {
      return Err(error.into());
    }
    anyhow::ensure!(output.status.success(), "{name} exited with {}", output.status);
    Ok(output.status)
  })
  .await?
}
