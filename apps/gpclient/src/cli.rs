use std::{env::temp_dir, fs::File};

use clap::{Parser, Subcommand};
use gpapi::utils::openssl;
use log::{info, LevelFilter};
use tempfile::NamedTempFile;

use crate::{
  connect::{ConnectArgs, ConnectHandler},
  disconnect::DisconnectHandler,
  launch_gui::{LaunchGuiArgs, LaunchGuiHandler},
};

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", compile_time::date_str!(), ")");

pub(crate) struct SharedArgs {
  pub(crate) fix_openssl: bool,
  pub(crate) ignore_tls_errors: bool,
}

#[derive(Subcommand)]
enum CliCommand {
  #[command(about = "Connect to a portal server")]
  Connect(Box<ConnectArgs>),
  #[command(about = "Disconnect from the server")]
  Disconnect,
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

  #[arg(long, help = "Uses extended compatibility mode for OpenSSL operations to support a broader range of systems and formats.")]
  fix_openssl: bool,
  #[arg(long, help = "Ignore the TLS errors")]
  ignore_tls_errors: bool,
}

impl Cli {
  fn fix_openssl(&self) -> anyhow::Result<Option<NamedTempFile>> {
    if self.fix_openssl {
      let file = openssl::fix_openssl_env()?;
      return Ok(Some(file));
    }

    Ok(None)
  }

  async fn run(&self) -> anyhow::Result<()> {
    // The temp file will be dropped automatically when the file handle is dropped
    // So, declare it here to ensure it's not dropped
    let _file = self.fix_openssl()?;
    let shared_args = SharedArgs {
      fix_openssl: self.fix_openssl,
      ignore_tls_errors: self.ignore_tls_errors,
    };

    if self.ignore_tls_errors {
      info!("TLS errors will be ignored");
    }

    match &self.command {
      CliCommand::Connect(args) => ConnectHandler::new(args, &shared_args).handle().await,
      CliCommand::Disconnect => DisconnectHandler::new().handle(),
      CliCommand::LaunchGui(args) => LaunchGuiHandler::new(args).handle().await,
    }
  }
}

fn init_logger(command: &CliCommand) {
  let mut builder = env_logger::builder();
  builder.filter_level(LevelFilter::Info);

  // Output the log messages to a file if the command is the auth callback
  if let CliCommand::LaunchGui(args) = command {
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

  init_logger(&cli.command);

  info!("gpclient started: {}", VERSION);

  if let Err(err) = cli.run().await {
    eprintln!("\nError: {}", err);

    let err = err.to_string();

    if err.contains("unsafe legacy renegotiation") && !cli.fix_openssl {
      eprintln!("\nRe-run it with the `--fix-openssl` option to work around this issue, e.g.:\n");
      // Print the command
      let args = std::env::args().collect::<Vec<_>>();
      eprintln!("{} --fix-openssl {}\n", args[0], args[1..].join(" "));
    }

    if err.contains("certificate verify failed") && !cli.ignore_tls_errors {
      eprintln!("\nRe-run it with the `--ignore-tls-errors` option to ignore the certificate error, e.g.:\n");
      // Print the command
      let args = std::env::args().collect::<Vec<_>>();
      eprintln!("{} --ignore-tls-errors {}\n", args[0], args[1..].join(" "));
    }

    std::process::exit(1);
  }
}
