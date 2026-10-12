use super::{
  command_runner::{ChildInput, run_observed},
  rendering_recovery::{RenderingCrash, StderrDiagnostic, eligible_with_inheritance, is_crash_exit},
};
use crate::service::transport::SessionCredential;
use anyhow::Context;
use std::{
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
) -> anyhow::Result<ExitStatus> {
  let retry_eligible = eligible_with_inheritance(&command, envs.is_none());
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
    let check = || {
      if cancellation.is_cancelled() {
        Err(std::io::Error::new(
          std::io::ErrorKind::Interrupted,
          "Desktop child launch cancelled",
        ))
      } else {
        Ok(())
      }
    };
    let mut diagnostic = StderrDiagnostic::default();
    let observed = run_observed(
      command,
      &check,
      None,
      &mut |bytes| diagnostic.push(bytes),
      &mut sink,
      ChildInput::Frame(&frame),
    )?;
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
