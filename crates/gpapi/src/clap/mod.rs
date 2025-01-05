use clap_verbosity_flag::{LogLevel, Verbosity, VerbosityFilter};
use log::Level;

use crate::error::PortalError;

pub mod args;

pub trait Args {
  fn fix_openssl(&self) -> bool;
  fn ignore_tls_errors(&self) -> bool;
}

pub fn handle_error(err: anyhow::Error, args: &impl Args) {
  eprintln!("\nError: {:?}", err);

  let Some(err) = err.downcast_ref::<PortalError>() else {
    return;
  };

  if err.is_legacy_openssl_error() && !args.fix_openssl() {
    eprintln!("\nRe-run it with the `--fix-openssl` option to work around this issue, e.g.:\n");
    let args = std::env::args().collect::<Vec<_>>();
    eprintln!("{} --fix-openssl {}\n", args[0], args[1..].join(" "));
  }

  if err.is_tls_error() && !args.ignore_tls_errors() {
    eprintln!("\nRe-run it with the `--ignore-tls-errors` option to ignore the certificate error, e.g.:\n");
    let args = std::env::args().collect::<Vec<_>>();
    eprintln!("{} --ignore-tls-errors {}\n", args[0], args[1..].join(" "));
  }
}

#[derive(Debug)]
pub struct InfoLevel;

pub type InfoLevelVerbosity = Verbosity<InfoLevel>;

impl LogLevel for InfoLevel {
  fn default_filter() -> VerbosityFilter {
    VerbosityFilter::Info
  }

  fn verbose_help() -> Option<&'static str> {
    Some("Enable verbose output, -v for debug, -vv for trace")
  }

  fn quiet_help() -> Option<&'static str> {
    Some("Decrease logging verbosity, -q for warnings, -qq for errors")
  }
}

pub trait ToVerboseArg {
  fn to_verbose_arg(&self) -> Option<&'static str>;
}

/// Convert the verbosity to the CLI argument value
/// The default verbosity is `Info`, which means no argument is needed
impl ToVerboseArg for InfoLevelVerbosity {
  fn to_verbose_arg(&self) -> Option<&'static str> {
    match self.filter() {
      VerbosityFilter::Off => Some("-qqq"),
      VerbosityFilter::Error => Some("-qq"),
      VerbosityFilter::Warn => Some("-q"),
      VerbosityFilter::Info => None,
      VerbosityFilter::Debug => Some("-v"),
      VerbosityFilter::Trace => Some("-vv"),
    }
  }
}

impl ToVerboseArg for Level {
  fn to_verbose_arg(&self) -> Option<&'static str> {
    match self {
      Level::Error => Some("-qq"),
      Level::Warn => Some("-q"),
      Level::Info => None,
      Level::Debug => Some("-v"),
      Level::Trace => Some("-vv"),
    }
  }
}
