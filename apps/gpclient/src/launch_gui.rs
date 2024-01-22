use std::{collections::HashMap, fs, path::PathBuf};

use clap::Args;
use directories::ProjectDirs;
use gpapi::{
  process::service_launcher::ServiceLauncher,
  utils::{endpoint::http_endpoint, env_file, shutdown_signal},
};
use log::info;

#[derive(Args)]
pub(crate) struct LaunchGuiArgs {
  #[arg(
    required = false,
    help = "The authentication data, used for the default browser authentication"
  )]
  auth_data: Option<String>,
  #[arg(long, help = "Launch the GUI minimized")]
  minimized: bool,
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
    let user = whoami::username();
    if user == "root" {
      anyhow::bail!("`launch-gui` cannot be run as root");
    }

    let auth_data = self.args.auth_data.as_deref().unwrap_or_default();
    if !auth_data.is_empty() {
      // Process the authentication data, its format is `globalprotectcallback:<data>`
      return feed_auth_data(auth_data).await;
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
    let env_file = env_file::persist_env_vars(Some(extra_envs))?;
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

async fn feed_auth_data(auth_data: &str) -> anyhow::Result<()> {
  let service_endpoint = http_endpoint().await?;

  reqwest::Client::default()
    .post(format!("{}/auth-data", service_endpoint))
    .json(&auth_data)
    .send()
    .await?
    .error_for_status()?;

  Ok(())
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

pub fn get_log_file() -> anyhow::Result<PathBuf> {
  let dirs = ProjectDirs::from("com.yuezk", "GlobalProtect-openconnect", "gpclient")
    .ok_or_else(|| anyhow::anyhow!("Failed to get project dirs"))?;

  fs::create_dir_all(dirs.data_dir())?;

  Ok(dirs.data_dir().join("gpclient.log"))
}
