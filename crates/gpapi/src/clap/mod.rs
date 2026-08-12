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
  /// Required rather than defaulted: a default would let an implementor drop
  /// the override and silently report failures as prose to a caller that asked
  /// for JSON, with nothing — not even a warning — to catch it.
  fn log_format(&self) -> LogFormat;
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

/// Write one message to `w` the way `format` asks for.
///
/// Text is emitted exactly as it always was, so existing output does not move.
/// JSON is written directly rather than through the `log` macros, because these
/// are reports rather than log lines: they must survive `--quiet`, which would
/// otherwise discard the only explanation of a failed run.
///
/// The record is rendered into one buffer and written once. `serde_json`'s
/// `Display` would otherwise reach the stream in dozens of fragments, and any
/// process sharing this stderr — the spawned authenticator, a connect script —
/// could land a line inside a half-written record.
fn write_message<W: Write>(w: &mut W, format: LogFormat, level: Level, message: &str) -> io::Result<()> {
  match format {
    LogFormat::Text => writeln!(w, "{message}"),
    LogFormat::Json => {
      // Callers pass the exact text line, blank-line padding included, so the
      // text form does not move. A record carries its own separation.
      let message = message.trim_start_matches('\n');
      let mut line = Vec::new();

      write_json_record(
        &mut line,
        &log::Record::builder()
          .level(level)
          .target(module_path!())
          .args(format_args!("{message}"))
          .build(),
      )?;

      w.write_all(&line)
    }
  }
}

/// Report a user-facing line that is not a log record — a note, a warning, or
/// advice about what to try next.
///
/// Callers used `eprintln!` directly, which in JSON mode drops prose into a
/// stream the caller is parsing a line at a time. Routing them here keeps one
/// answer to "how does this program emit a user-facing line", and leaves the
/// text form byte-for-byte unchanged.
pub fn report(format: LogFormat, level: Level, message: &str) {
  let _ = write_message(&mut io::stderr().lock(), format, level, message);
}

fn write_failure<W: Write>(w: &mut W, format: LogFormat, err: &anyhow::Error, hints: &[String]) -> io::Result<()> {
  match format {
    LogFormat::Text => {
      writeln!(w, "\nError: {err:?}")?;

      for hint in hints {
        writeln!(w, "\n{hint}\n")?;
      }
    }
    LogFormat::Json => {
      write_message(w, format, Level::Error, &format!("{err:?}"))?;

      for hint in hints {
        write_message(w, format, Level::Error, hint)?;
      }
    }
  }

  Ok(())
}

fn handle_error_to<W: Write>(w: &mut W, err: anyhow::Error, args: &impl Args) -> io::Result<()> {
  let hints = retry_hints(&err, args);

  write_failure(w, args.log_format(), &err, &hints)
}

pub fn handle_error(err: anyhow::Error, args: &impl Args) {
  // Nothing useful to do if stderr itself is gone.
  let _ = handle_error_to(&mut io::stderr().lock(), err, args);
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
    log_format: LogFormat,
  }

  impl Args for TestArgs {
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

  /// The seam that matters: `handle_error` must ask the args which format to
  /// use. Testing the writer alone would leave a version that always reports as
  /// prose passing every test.
  #[test]
  fn handle_error_honours_the_requested_format() {
    let args = TestArgs {
      log_format: LogFormat::Json,
      ..Default::default()
    };
    let mut out = Vec::new();

    handle_error_to(&mut out, anyhow!(PortalError::TlsError), &args).expect("writing to a Vec cannot fail");

    let out = String::from_utf8(out).expect("output should be UTF-8");
    for line in out.lines() {
      serde_json::from_str::<serde_json::Value>(line).unwrap_or_else(|e| panic!("not JSON: {line:?} ({e})"));
    }
    assert!(!out.is_empty(), "the failure must be reported");
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

  fn failure_output(format: LogFormat) -> String {
    let hints = vec!["Re-run it with the `--ignore-tls-errors` option".to_string()];
    let mut out = Vec::new();

    write_failure(&mut out, format, &anyhow!(PortalError::TlsError), &hints).expect("writing to a Vec cannot fail");

    String::from_utf8(out).expect("output should be UTF-8")
  }

  /// The failure is the program's final report, so in JSON mode it must be one
  /// more record on the same stream — and it must not be filtered away, which
  /// is why it does not go through the log macros. A run that fails silently is
  /// worse than one that fails noisily.
  #[test]
  fn the_failure_is_reported_as_records() {
    let out = failure_output(LogFormat::Json);
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines.len(), 2, "expected the error and its hint: {out:?}");

    for line in lines {
      let v: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON: {line:?} ({e})"));
      assert_eq!(v["level"], "ERROR");
    }
  }

  /// The format has to be honoured, not merely available: reporting the failure
  /// as prose to a caller that asked for JSON is the defect this whole flag
  /// exists to remove.
  #[test]
  fn the_format_decides_how_the_failure_is_written() {
    assert!(
      serde_json::from_str::<serde_json::Value>(failure_output(LogFormat::Json).lines().next().unwrap()).is_ok(),
      "json mode must not emit prose"
    );

    let text = failure_output(LogFormat::Text);
    assert!(text.starts_with("\nError: "), "text mode keeps its shape: {text:?}");
    assert!(
      serde_json::from_str::<serde_json::Value>(text.trim()).is_err(),
      "text mode must not emit JSON: {text:?}"
    );
  }
}
