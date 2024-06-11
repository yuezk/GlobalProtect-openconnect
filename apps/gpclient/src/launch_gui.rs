use std::{collections::HashMap, env::temp_dir, fs, path::PathBuf};

use clap::Args;
use directories::ProjectDirs;
use gpapi::{
  process::service_launcher::ServiceLauncher,
  utils::{endpoint::http_endpoint, env_file, shutdown_signal},
};
use log::info;
use tokio::io::AsyncWriteExt;

use crate::GP_CLIENT_PORT_FILE;

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
  let _ = tokio::join!(feed_auth_data_gui(auth_data), feed_auth_data_cli(auth_data));

  // Cleanup the temporary file
  let html_file = temp_dir().join("gpauth.html");
  let _ = std::fs::remove_file(html_file);

  Ok(())
}

async fn feed_auth_data_gui(auth_data: &str) -> anyhow::Result<()> {
  let service_endpoint = http_endpoint().await?;

  reqwest::Client::default()
    .post(format!("{}/auth-data", service_endpoint))
    .body(auth_data.to_string())
    .send()
    .await?
    .error_for_status()?;

  Ok(())
}

async fn feed_auth_data_cli(auth_data: &str) -> anyhow::Result<()> {
  let port = tokio::fs::read_to_string(GP_CLIENT_PORT_FILE).await?;
  let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port.trim())).await?;

  stream.write_all(auth_data.as_bytes()).await?;

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
