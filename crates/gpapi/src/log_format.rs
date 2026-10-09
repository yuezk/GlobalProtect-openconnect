//! Machine-readable log output.
//!
//! The default human format is fine at a terminal, but tools that drive
//! gpclient headless have to parse it — and end up matching on prose, including
//! text that comes from dependencies rather than from this project. JSON lines
//! give those callers fields instead.
//!
//! This module deliberately depends only on `log` and `serde_json`, so every
//! binary can use it without pulling in a logging backend, and so the rendering
//! can be tested without installing a global logger.

use std::io::{self, Write};

use chrono::{SecondsFormat, Utc};
use serde_json::json;

/// How log records are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum LogFormat {
  /// The human-readable default.
  #[default]
  Text,
  /// One JSON object per line.
  Json,
}

impl LogFormat {
  /// The name this format is known by on a command line.
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Text => "text",
      Self::Json => "json",
    }
  }
}

/// Write a [`log::Record`] to `w` as one JSON line, newline included.
///
/// This is the form a logging backend needs, and it writes into the buffer the
/// backend already handed us rather than allocating a line to be copied. It
/// takes a `Record` and a `Write` rather than any backend's own types, so `log`
/// stays the only logging dependency here — with `env_logger`, this function
/// *is* the format callback:
///
/// ```ignore
/// // Not compiled as a doctest: env_logger is an optional dependency of this
/// // crate, so it is not linked when the docs are built.
/// env_logger::builder().format(write_json_record).init();
/// ```
pub fn write_json_record<W: Write>(w: &mut W, record: &log::Record) -> io::Result<()> {
  writeln!(
    w,
    "{}",
    json!({
      "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
      "level": record.level().as_str(),
      "target": record.target(),
      "message": record.args().to_string(),
    })
  )
}

#[cfg(test)]
mod tests {
  use chrono::{DateTime, SubsecRound};
  use log::Level;

  use super::*;

  fn parse(line: &str) -> serde_json::Value {
    serde_json::from_str(line).expect("each line must be valid JSON")
  }

  fn write_record(record: &log::Record) -> String {
    let mut buf = Vec::new();
    write_json_record(&mut buf, record).expect("writing to a Vec cannot fail");
    String::from_utf8(buf).expect("output should be UTF-8")
  }

  /// The fields a consumer is promised, taken off a `Record` as a backend
  /// hands it over.
  #[test]
  fn record_renders_the_expected_fields() {
    // Built inline: `format_args!` borrows its arguments for the statement it
    // appears in, so a `Record` held in a `let` would outlive them.
    let v = parse(&write_record(
      &log::Record::builder()
        .level(Level::Error)
        .target("gpclient::connect")
        .args(format_args!("portal {} unreachable", "vpn.example.com"))
        .build(),
    ));

    assert_eq!(v["level"], "ERROR");
    assert_eq!(v["target"], "gpclient::connect");
    assert_eq!(v["message"], "portal vpn.example.com unreachable");
  }

  /// Without a timestamp the JSON output would be strictly less informative
  /// than the text format it replaces, and a consumer could not order or
  /// correlate events.
  #[test]
  fn records_carry_a_utc_timestamp() {
    let before = Utc::now();
    let v = parse(&write_record(
      &log::Record::builder()
        .level(Level::Info)
        .target("t")
        .args(format_args!("connected"))
        .build(),
    ));
    let after = Utc::now();

    let ts = v["timestamp"].as_str().expect("timestamp must be a string");
    let parsed = DateTime::parse_from_rfc3339(ts)
      .unwrap_or_else(|e| panic!("timestamp must be RFC 3339: {ts:?} ({e})"))
      .with_timezone(&Utc);

    assert!(ts.ends_with('Z'), "timestamp must be UTC, got {ts:?}");
    assert!(
      parsed >= before.trunc_subsecs(3) && parsed <= after,
      "timestamp {parsed} should be between {before} and {after}"
    );
  }

  /// The backend appends records to one stream, so each has to terminate its
  /// own line — exactly one, or the next record joins this one. A message
  /// containing a newline must be escaped into the JSON string rather than
  /// splitting the record in two.
  #[test]
  fn record_ends_with_a_single_newline() {
    let out = write_record(
      &log::Record::builder()
        .level(Level::Info)
        .target("t")
        .args(format_args!("first\nsecond"))
        .build(),
    );

    assert!(out.ends_with('\n'), "record must terminate its line: {out:?}");
    assert_eq!(out.matches('\n').count(), 1, "record must be one line: {out:?}");
    assert_eq!(parse(&out)["message"], "first\nsecond", "the newline must survive");
  }

  /// `as_str` is what gets passed to a child process on the command line, and
  /// clap parses that with the names from its own `ValueEnum` derive. If the
  /// two ever disagree, gpclient hands gpauth a value gpauth rejects.
  #[cfg(feature = "clap")]
  #[test]
  fn as_str_agrees_with_the_names_clap_parses() {
    use clap::ValueEnum;

    for format in [LogFormat::Text, LogFormat::Json] {
      let clap_name = format
        .to_possible_value()
        .expect("every variant is selectable")
        .get_name()
        .to_owned();

      assert_eq!(format.as_str(), clap_name);
    }
  }
}
