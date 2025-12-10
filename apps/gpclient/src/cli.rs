use std::{env::temp_dir, fs::File, str::FromStr};

use anyhow::bail;
use clap::{Parser, Subcommand};
use gpapi::{
  clap::{Args, InfoLevelVerbosity, handle_error},
  utils::openssl,
};
use log::info;
use sysinfo::{Pid, System};
use tempfile::NamedTempFile;
use tokio::fs;

use crate::{
  GP_CLIENT_LOCK_FILE,
  connect::{ConnectArgs, ConnectHandler},
  disconnect::{DisconnectArgs, DisconnectHandler},
  hip::{HipArgs, HipHandler},
  launch_gui::{LaunchGuiArgs, LaunchGuiHandler},
};

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", compile_time::date_str!(), ")");

pub(crate) struct SharedArgs<'a> {
  pub(crate) fix_openssl: bool,
  pub(crate) ignore_tls_errors: bool,
  pub(crate) verbose: &'a InfoLevelVerbosity,
}

#[derive(Subcommand)]
enum CliCommand {
  #[command(about = "Connect to a portal server")]
  Connect(Box<ConnectArgs>),
  #[command(about = "Disconnect from the server")]
  Disconnect(DisconnectArgs),
  #[command(about = "Launch the GUI")]
  LaunchGui(LaunchGuiArgs),
  #[command(about = "Generate HIP report")]
  Hip(HipArgs),
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
    let Ok(c) = fs::read_to_string(GP_CLIENT_LOCK_FILE).await else {
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
    // check if an instance is running
    if !matches!(self.command, CliCommand::Disconnect(_)) && self.is_running().await {
      bail!("Another instance of the client is already running");
    }

    // The temp file will be dropped automatically when the file handle is dropped
    // So, declare it here to ensure it's not dropped
    let _file = self.fix_openssl()?;
    let shared_args = SharedArgs {
      fix_openssl: self.fix_openssl,
      ignore_tls_errors: self.ignore_tls_errors,
      verbose: &self.verbose,
    };

    if self.ignore_tls_errors {
      info!("TLS errors will be ignored");
    }

    match &self.command {
      CliCommand::Connect(args) => ConnectHandler::new(args, &shared_args).handle().await,
      CliCommand::Disconnect(args) => DisconnectHandler::new(args).handle().await,
      CliCommand::LaunchGui(args) => LaunchGuiHandler::new(args).handle().await,
      CliCommand::Hip(args) => HipHandler::new(args).handle().await,
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
