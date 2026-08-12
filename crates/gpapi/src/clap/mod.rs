use std::io::{self, Write};

use clap_verbosity_flag::{LogLevel, Verbosity, VerbosityFilter};
use log::Level;

use crate::{
  error::PortalError,
  log_format::{LogFormat, write_json_record},
};

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

/// Render the failure as JSON records.
///
/// Written directly rather than through the `log` macros: this is the program's
/// final report, not a log line, and it must survive `--quiet` exactly as the
/// text form does. Going through the logger would let `-qqq` discard the only
/// explanation of why the run failed.
fn write_error_records<W: Write>(w: &mut W, err: &anyhow::Error, hints: &[String]) -> io::Result<()> {
  let mut write_one = |message: &str| {
    write_json_record(
      w,
      &log::Record::builder()
        .level(Level::Error)
        .target(module_path!())
        .args(format_args!("{message}"))
        .build(),
    )
  };

  write_one(&format!("{err:?}"))?;

  for hint in hints {
    write_one(hint)?;
  }

  Ok(())
}

pub fn handle_error(err: anyhow::Error, args: &impl Args) {
  let hints = retry_hints(&err, args);

  // In JSON mode the failure has to arrive as records too: a block of prose in
  // the middle of the stream would break a caller parsing it a line at a time,
  // and it would break on the record explaining the failure.
  if args.log_format() == LogFormat::Json {
    // Nothing useful to do if stderr itself is gone.
    let _ = write_error_records(&mut io::stderr().lock(), &err, &hints);
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

  /// The failure is the program's final report, so in JSON mode it must be one
  /// more record on the same stream — and it must not be filtered away, which
  /// is why it does not go through the log macros. A run that fails silently is
  /// worse than one that fails noisily.
  #[test]
  fn the_failure_is_reported_as_records() {
    let hints = vec!["Re-run it with the `--ignore-tls-errors` option".to_string()];
    let mut out = Vec::new();

    write_error_records(&mut out, &anyhow!(PortalError::TlsError), &hints).expect("writing to a Vec cannot fail");

    let out = String::from_utf8(out).expect("output should be UTF-8");
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines.len(), 2, "expected the error and its hint: {out:?}");

    for line in lines {
      let v: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON: {line:?} ({e})"));
      assert_eq!(v["level"], "ERROR");
    }
  }
}
