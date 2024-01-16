use clap::{Parser, Subcommand};
use gpapi::utils::openssl;
use log::{info, LevelFilter};
use tempfile::NamedTempFile;

use crate::{
  connect::{ConnectArgs, ConnectHandler},
  disconnect::DisconnectHandler,
  launch_gui::{LaunchGuiArgs, LaunchGuiHandler},
};

const VERSION: &str = concat!(
  env!("CARGO_PKG_VERSION"),
  " (",
  compile_time::date_str!(),
  ")"
);

#[derive(Subcommand)]
enum CliCommand {
  #[command(about = "Connect to a portal server")]
  Connect(ConnectArgs),
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
"
)]
struct Cli {
  #[command(subcommand)]
  command: CliCommand,

  #[arg(
    long,
    help = "Get around the OpenSSL `unsafe legacy renegotiation` error"
  )]
  fix_openssl: bool,
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

    match &self.command {
      CliCommand::Connect(args) => ConnectHandler::new(args, self.fix_openssl).handle().await,
      CliCommand::Disconnect => DisconnectHandler::new().handle(),
      CliCommand::LaunchGui(args) => LaunchGuiHandler::new(args).handle().await,
    }
  }
}

fn init_logger() {
  env_logger::builder().filter_level(LevelFilter::Info).init();
}

pub(crate) async fn run() {
  let cli = Cli::parse();

  init_logger();
  info!("gpclient started: {}", VERSION);

  if let Err(err) = cli.run().await {
    eprintln!("\nError: {}", err);

    if err.to_string().contains("unsafe legacy renegotiation") && !cli.fix_openssl {
      eprintln!("\nRe-run it with the `--fix-openssl` option to work around this issue, e.g.:\n");
      // Print the command
      let args = std::env::args().collect::<Vec<_>>();
      eprintln!("{} --fix-openssl {}\n", args[0], args[1..].join(" "));
    }

    std::process::exit(1);
  }
}
