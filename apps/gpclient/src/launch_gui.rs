use std::{collections::HashMap, fs, path::PathBuf};

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", test))]
use std::process::{Command, ExitStatus};

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
use anyhow::Context;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", test))]
use anyhow::bail;
use clap::Args;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", test))]
use common::binary_paths;
use directories::ProjectDirs;
use gpapi::{
  process::service_launcher::ServiceLauncher,
  utils::{endpoint::http_endpoint, env_utils, shutdown_signal},
};
use log::info;

#[derive(Args)]
pub(crate) struct LaunchGuiArgs {
  #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
  #[arg(
    required = false,
    conflicts_with = "minimized",
    value_parser = parse_auth_callback,
    help = "The external-browser authentication callback URL"
  )]
  callback: Option<String>,
  #[arg(long, help = "Launch the GUI minimized")]
  minimized: bool,
}

impl LaunchGuiArgs {
  pub(crate) fn is_auth_callback(&self) -> bool {
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    {
      return self.callback.is_some();
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
    false
  }
}

pub(crate) struct LaunchGuiHandler<'a> {
  args: &'a LaunchGuiArgs,
}

impl<'a> LaunchGuiHandler<'a> {
  pub(crate) fn new(args: &'a LaunchGuiArgs) -> Self {
    Self { args }
  }

  pub(crate) async fn handle(&self) -> anyhow::Result<()> {
    // `launch-gui`cannot be run as root
    let user = whoami::username().map_err(|err| anyhow::anyhow!("Failed to resolve current user: {err}"))?;
    if user == "root" {
      anyhow::bail!("`launch-gui` cannot be run as root");
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    if let Some(callback) = &self.args.callback {
      return forward_auth_callback(callback);
    }

    if try_active_gui().await.is_ok() {
      info!("The GUI is already running");
      return Ok(());
    }

    tokio::spawn(async move {
      shutdown_signal().await;
      info!("Shutting down...");
    });

    let log_file = get_log_file()?;
    let log_file_path = log_file.to_string_lossy().to_string();

    info!("Log file: {}", log_file_path);

    let mut extra_envs = HashMap::<String, String>::new();
    extra_envs.insert("GP_LOG_FILE".into(), log_file_path.clone());

    // Persist the environment variables to a file
    let env_file = env_utils::persist_env_vars(Some(extra_envs))?;
    let env_file = env_file.into_temp_path();
    let env_file_path = env_file.to_string_lossy().to_string();

    let exit_status = ServiceLauncher::new()
      .minimized(self.args.minimized)
      .env_file(&env_file_path)
      .log_file(&log_file_path)
      .launch()
      .await?;

    info!("Service exited with status: {}", exit_status);

    Ok(())
  }
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
fn forward_auth_callback(callback: &str) -> anyhow::Result<()> {
  let status = auth_callback_command(callback)
    .status()
    .context("Failed to start the authentication callback handler")?;
  ensure_auth_callback_succeeded(status)
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", test))]
fn auth_callback_command(callback: &str) -> Command {
  let mut command = Command::new(binary_paths::gpauth());
  command.args(["auth-callback", callback]);
  command
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", test))]
fn ensure_auth_callback_succeeded(status: ExitStatus) -> anyhow::Result<()> {
  if !status.success() {
    bail!("Authentication callback handler exited with {status}");
  }

  Ok(())
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", test))]
fn parse_auth_callback(callback: &str) -> Result<String, String> {
  callback
    .starts_with("globalprotectcallback:")
    .then(|| callback.to_owned())
    .ok_or_else(|| "callback URL must use the globalprotectcallback scheme".to_owned())
}

async fn try_active_gui() -> anyhow::Result<()> {
  let service_endpoint = http_endpoint().await?;

  reqwest::Client::default()
    .post(format!("{}/active-gui", service_endpoint))
    .send()
    .await?
    .error_for_status()?;

  Ok(())
}

fn get_log_file() -> anyhow::Result<PathBuf> {
  let dirs = ProjectDirs::from("com.yuezk", "GlobalProtect-openconnect", "gpclient")
    .ok_or_else(|| anyhow::anyhow!("Failed to get project dirs"))?;

  fs::create_dir_all(dirs.data_dir())?;

  Ok(dirs.data_dir().join("gpclient.log"))
}

#[cfg(all(test, unix))]
mod tests {
  use std::os::unix::process::ExitStatusExt;

  use super::*;

  const CALLBACK: &str = "globalprotectcallback:cas-as=1&un=alice@example.com&token=token";

  #[test]
  fn callback_command_preserves_callback_argument() {
    let command = auth_callback_command(CALLBACK);
    let args = command
      .get_args()
      .map(|argument| argument.to_str().unwrap())
      .collect::<Vec<_>>();

    assert_eq!(args, ["auth-callback", CALLBACK]);
  }

  #[test]
  fn callback_command_status_is_propagated() {
    assert!(ensure_auth_callback_succeeded(ExitStatus::from_raw(0)).is_ok());
    assert!(ensure_auth_callback_succeeded(ExitStatus::from_raw(1 << 8)).is_err());
  }

  #[test]
  fn rejects_non_callback_url() {
    assert!(parse_auth_callback("https://example.com/callback").is_err());
  }
}
