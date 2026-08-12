use clap_verbosity_flag::{LogLevel, Verbosity, VerbosityFilter};
use log::{Level, error};

use crate::{error::PortalError, log_format::LogFormat};

pub mod args;

pub trait Args {
  fn fix_openssl(&self) -> bool;
  fn ignore_tls_errors(&self) -> bool;

  /// How this program renders its logs.
  ///
  /// Defaults to text so that a binary without the flag is unaffected.
  fn log_format(&self) -> LogFormat {
    LogFormat::Text
  }
}

/// Suggestions for re-running the command, for failures that have a known
/// workaround. Empty when there is nothing useful to say — including when the
/// relevant option is already in use.
fn retry_hints(err: &anyhow::Error, args: &impl Args) -> Vec<String> {
  let Some(err) = err.downcast_ref::<PortalError>() else {
    return Vec::new();
  };

  let command_line = || {
    let argv = std::env::args().collect::<Vec<_>>();
    let program = argv.first().cloned().unwrap_or_default();
    let rest = argv.get(1..).map(|rest| rest.join(" ")).unwrap_or_default();

    (program, rest)
  };

  // The blank line between the sentence and the command is part of the output
  // users already see; keep it exactly.
  let mut hints = Vec::new();

  if err.is_legacy_openssl_error() && !args.fix_openssl() {
    let (program, rest) = command_line();
    hints.push(format!(
      "Re-run it with the `--fix-openssl` option to work around this issue, e.g.:\n\n{program} --fix-openssl {rest}"
    ));
  }

  if err.is_tls_error() && !args.ignore_tls_errors() {
    let (program, rest) = command_line();
    hints.push(format!(
      "Re-run it with the `--ignore-tls-errors` option to ignore the certificate error, e.g.:\n\n{program} --ignore-tls-errors {rest}"
    ));
  }

  hints
}

pub fn handle_error(err: anyhow::Error, args: &impl Args) {
  let hints = retry_hints(&err, args);

  // In JSON mode this has to go through the logger like everything else:
  // writing it straight to stderr would put a block of prose in the middle of
  // a stream the caller is parsing a line at a time — and it would land on the
  // record that explains the failure.
  if args.log_format() == LogFormat::Json {
    error!("{err:?}");

    for hint in hints {
      error!("{hint}");
    }

    return;
  }

  eprintln!("\nError: {err:?}");

  for hint in hints {
    eprintln!("\n{hint}\n");
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

#[cfg(test)]
mod tests {
  use anyhow::anyhow;

  use super::*;

  #[derive(Default)]
  struct TestArgs {
    fix_openssl: bool,
    ignore_tls_errors: bool,
  }

  impl Args for TestArgs {
    fn fix_openssl(&self) -> bool {
      self.fix_openssl
    }

    fn ignore_tls_errors(&self) -> bool {
      self.ignore_tls_errors
    }
  }

  /// A binary that never learned about the flag keeps its current output.
  #[test]
  fn log_format_defaults_to_text() {
    assert_eq!(TestArgs::default().log_format(), LogFormat::Text);
  }

  #[test]
  fn a_tls_failure_suggests_ignoring_tls_errors() {
    let hints = retry_hints(&anyhow!(PortalError::TlsError), &TestArgs::default());

    assert_eq!(hints.len(), 1, "expected one hint, got {hints:?}");
    assert!(hints[0].contains("--ignore-tls-errors"), "got {:?}", hints[0]);
    // The command sits on its own line after a blank one, as it always has.
    assert!(
      hints[0].contains("e.g.:\n\n"),
      "blank line before the command: {:?}",
      hints[0]
    );
  }

  /// Suggesting an option the user already passed is noise, and in JSON mode it
  /// would be an extra record on every failed run.
  #[test]
  fn no_hint_when_the_option_is_already_in_use() {
    let args = TestArgs {
      ignore_tls_errors: true,
      ..Default::default()
    };

    assert!(retry_hints(&anyhow!(PortalError::TlsError), &args).is_empty());
  }

  /// Most failures have no known workaround; inventing advice for them would
  /// mislead.
  #[test]
  fn no_hints_for_an_unrelated_error() {
    let hints = retry_hints(&anyhow!("something else went wrong"), &TestArgs::default());

    assert!(hints.is_empty(), "got {hints:?}");
  }
}
