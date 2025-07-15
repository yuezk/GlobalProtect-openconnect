use std::{env::temp_dir, fs::File, path::PathBuf, str::FromStr};

use anyhow::bail;
use clap::{Parser, Subcommand};
use gpapi::{
  clap::{handle_error, Args, InfoLevelVerbosity},
  utils::{openssl, runtime},
};
use log::info;
use sysinfo::{Pid, System};
use tempfile::NamedTempFile;
use tokio::fs;

use crate::{
  connect::{ConnectArgs, ConnectHandler},
  disconnect::{DisconnectArgs, DisconnectHandler},
  launch_gui::{LaunchGuiArgs, LaunchGuiHandler},
};

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", compile_time::date_str!(), ")");

pub(crate) struct SharedArgs<'a> {
  pub(crate) fix_openssl: bool,
  pub(crate) ignore_tls_errors: bool,
  pub(crate) verbose: &'a InfoLevelVerbosity,
  pub(crate) lock_file_path: PathBuf,
}

#[derive(Subcommand)]
enum CliCommand {
  #[command(about = "Connect to a portal server")]
  Connect(Box<ConnectArgs>),
  #[command(about = "Disconnect from the server")]
  Disconnect(DisconnectArgs),
  #[command(about = "Launch the GUI")]
  LaunchGui(LaunchGuiArgs),
}

#[derive(Parser)]
#[command(
  version = VERSION,
  author,
  about = "The GlobalProtect VPN client, based on OpenConnect, supports the SSO authentication method.",
  help_template = "\
{before-help}{name} {version}
{author}

{about}

{usage-heading} {usage}

{all-args}{after-help}

See 'gpclient help <command>' for more information on a specific command.
"
)]
struct Cli {
  #[command(subcommand)]
  command: CliCommand,

  #[arg(
    long,
    help = "Uses extended compatibility mode for OpenSSL operations to support a broader range of systems and formats."
  )]
  fix_openssl: bool,
  #[arg(long, help = "Ignore the TLS errors")]
  ignore_tls_errors: bool,

  #[command(flatten)]
  verbose: InfoLevelVerbosity,
}

impl Args for Cli {
  fn fix_openssl(&self) -> bool {
    self.fix_openssl
  }

  fn ignore_tls_errors(&self) -> bool {
    self.ignore_tls_errors
  }
}

impl Cli {
  async fn is_running(&self) -> bool {
    let lock_file_path = match runtime::get_client_lock_path() {
      Ok(path) => path,
      Err(_) => return false,
    };

    let Ok(c) = fs::read_to_string(&lock_file_path).await else {
      return false;
    };

    let Ok(pid) = Pid::from_str(c.trim()) else {
      return false;
    };

    let s = System::new_all();
    let Some(p) = s.process(pid) else {
      return false;
    };

    p.exe()
      .map(|exe| exe.to_string_lossy().contains("gpclient"))
      .unwrap_or(false)
  }

  fn fix_openssl(&self) -> anyhow::Result<Option<NamedTempFile>> {
    if self.fix_openssl {
      let file = openssl::fix_openssl_env()?;
      return Ok(Some(file));
    }

    Ok(None)
  }

  async fn run(&self) -> anyhow::Result<()> {
    // Determine appropriate lock file path based on user privileges
    let lock_file_path =
      runtime::get_client_lock_path().map_err(|e| anyhow::anyhow!("Failed to determine lock file path: {}", e))?;

    info!("Using lock file: {}", lock_file_path.display());

    // Check if we have permission to use this lock file path
    if let Err(e) = runtime::ensure_lock_file_accessible(&lock_file_path) {
      bail!("Cannot access lock file: {}", e);
    }

    // check if an instance is running
    if self.is_running().await {
      bail!("Another instance of the client is already running");
    }

    // The temp file will be dropped automatically when the file handle is dropped
    // So, declare it here to ensure it's not dropped
    let _file = self.fix_openssl()?;
    let shared_args = SharedArgs {
      fix_openssl: self.fix_openssl,
      ignore_tls_errors: self.ignore_tls_errors,
      verbose: &self.verbose,
      lock_file_path,
    };

    if self.ignore_tls_errors {
      info!("TLS errors will be ignored");
    }

    match &self.command {
      CliCommand::Connect(args) => ConnectHandler::new(args, &shared_args).handle().await,
      CliCommand::Disconnect(args) => DisconnectHandler::new(args).handle().await,
      CliCommand::LaunchGui(args) => LaunchGuiHandler::new(args).handle().await,
    }
  }
}

fn init_logger(cli: &Cli) {
  let mut builder = env_logger::builder();
  builder.filter_level(cli.verbose.log_level_filter());

  // Output the log messages to a file if the command is the auth callback
  if let CliCommand::LaunchGui(args) = &cli.command {
    let auth_data = args.auth_data.as_deref().unwrap_or_default();
    if !auth_data.is_empty() {
      if let Ok(log_file) = File::create(temp_dir().join("gpcallback.log")) {
        let target = Box::new(log_file);
        builder.target(env_logger::Target::Pipe(target));
      }
    }
  }

  builder.init();
}

pub(crate) async fn run() {
  let cli = Cli::parse();

  init_logger(&cli);

  info!("gpclient started: {}", VERSION);

  if let Err(err) = cli.run().await {
    handle_error(err, &cli);
    std::process::exit(1);
  }
}
