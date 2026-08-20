use std::{env::temp_dir, fs::File, str::FromStr};

use anyhow::bail;
use clap::{Parser, Subcommand};
use gpapi::{
  clap::{Args, InfoLevelVerbosity, handle_error},
  log_format::{LogFormat, write_json_record},
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

const VERSION: &str = concat!(
  env!("CARGO_PKG_VERSION"),
  " (",
  env!("GPCLIENT_GIT_COMMIT"),
  " ",
  compile_time::date_str!(),
  ")"
);

pub(crate) struct SharedArgs<'a> {
  pub(crate) fix_openssl: bool,
  pub(crate) ignore_tls_errors: bool,
  pub(crate) verbose: &'a InfoLevelVerbosity,
  pub(crate) log_format: LogFormat,
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

  #[arg(
    long,
    value_enum,
    default_value_t = LogFormat::Text,
    help = "Log output format. JSON is intended for non-interactive consumers; interactive prompts remain text."
  )]
  log_format: LogFormat,

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

  fn log_format(&self) -> LogFormat {
    self.log_format
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
      log_format: self.log_format,
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

fn build_logger(cli: &Cli) -> env_logger::Builder {
  let mut builder = env_logger::builder();
  builder.filter_level(cli.verbose.log_level_filter());

  // Text is env_logger's own format, so leave it untouched rather than
  // reimplementing it.
  if cli.log_format == LogFormat::Json {
    builder.format(write_json_record);
  }

  // Output the log messages to a file if the command is the auth callback
  if let CliCommand::LaunchGui(args) = &cli.command {
    let auth_data = args.auth_data.as_deref().unwrap_or_default();
    if !auth_data.is_empty()
      && let Ok(log_file) = File::create(temp_dir().join("gpcallback.log"))
    {
      let target = Box::new(log_file);
      builder.target(env_logger::Target::Pipe(target));
    }
  }

  builder
}

pub(crate) async fn run() {
  let cli = Cli::parse();

  build_logger(&cli).init();

  info!("gpclient started: {}", VERSION);

  if let Err(err) = cli.run().await {
    handle_error(err, &cli);
    std::process::exit(1);
  }
}

#[cfg(test)]
mod tests {
  use std::{
    io::Write,
    sync::{Arc, Mutex},
  };

  use log::Log;

  use super::*;

  fn parse_cli(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).expect("arguments should parse")
  }

  /// Text stays the default: a plain `gpclient connect` must keep the output it
  /// has always had, or every existing user's terminal changes under them.
  #[test]
  fn log_format_defaults_to_text() {
    assert_eq!(parse_cli(&["gpclient", "disconnect"]).log_format, LogFormat::Text);
  }

  #[test]
  fn log_format_accepts_json() {
    assert_eq!(
      parse_cli(&["gpclient", "--log-format", "json", "disconnect"]).log_format,
      LogFormat::Json
    );
  }

  /// An unknown value is a mistake worth reporting rather than silently
  /// falling back to text, which would leave a caller parsing prose.
  #[test]
  fn log_format_rejects_an_unknown_value() {
    // `.err()` rather than `unwrap_err()`: the Ok side is `Cli`, which is not
    // `Debug`, and it is not worth deriving it just to print an error we expect.
    let err = Cli::try_parse_from(["gpclient", "--log-format", "yaml", "disconnect"])
      .err()
      .expect("an unknown log format should be rejected");
    let msg = err.to_string();
    assert!(msg.contains("yaml"), "error should name the input: {msg}");
    assert!(msg.contains("json"), "error should list the valid values: {msg}");
  }

  /// A `Write` that keeps what was written, so the logger can be driven for
  /// real instead of asserting on how it was configured.
  #[derive(Clone, Default)]
  struct Captured(Arc<Mutex<Vec<u8>>>);

  impl Captured {
    fn contents(&self) -> String {
      String::from_utf8(self.0.lock().unwrap().clone()).expect("log output should be UTF-8")
    }
  }

  impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
      self.0.lock().unwrap().extend_from_slice(buf);
      Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
      Ok(())
    }
  }

  fn log_one_record(args: &[&str]) -> String {
    let captured = Captured::default();
    let logger = build_logger(&parse_cli(args))
      .target(env_logger::Target::Pipe(Box::new(captured.clone())))
      .build();

    // Drive the logger directly: `init()` installs a global that a second test
    // could not replace.
    logger.log(
      &log::Record::builder()
        .level(log::Level::Info)
        .target("gpclient")
        .args(format_args!("connected to {}", "vpn.example.com"))
        .build(),
    );
    logger.flush();

    captured.contents()
  }

  /// The point of the flag: with `json`, a caller gets fields, and gets them one
  /// record per line so the stream can be read incrementally.
  #[test]
  fn json_format_emits_one_json_object_per_record() {
    let out = log_one_record(&["gpclient", "--log-format", "json", "disconnect"]);

    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines.len(), 1, "expected exactly one line, got: {out:?}");

    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap_or_else(|e| panic!("not JSON: {out:?} ({e})"));
    assert_eq!(v["level"], "INFO");
    assert_eq!(v["target"], "gpclient");
    assert_eq!(v["message"], "connected to vpn.example.com");
  }

  #[test]
  fn text_format_is_left_alone() {
    let out = log_one_record(&["gpclient", "disconnect"]);

    assert!(out.contains("connected to vpn.example.com"), "message missing: {out:?}");
    assert!(
      serde_json::from_str::<serde_json::Value>(out.trim()).is_err(),
      "text output should not be JSON: {out:?}"
    );
  }
}
