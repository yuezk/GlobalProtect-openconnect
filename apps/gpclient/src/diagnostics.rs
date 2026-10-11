use std::{
  collections::BTreeMap, fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf, process::Stdio,
};

use anyhow::{Context, ensure};
use clap::Args;
use common::binary_paths;
use serde::Serialize;
use tokio::{io::AsyncWriteExt, process::Command};

const COLLECTOR: &str = include_str!("../../../scripts/collect-diagnostics.sh");
const MAX_REPORT_BYTES: usize = 64 * 1024;
const FIELDS: &[&str] = &[
  "os",
  "os_version",
  "kernel",
  "architecture",
  "execution_user",
  "ssh_session",
  "virtualization",
  "gpclient_path",
  "gpclient_version",
  "gpauth_path",
  "gpauth_version",
  "gpgui_path",
  "gpgui_version",
  "display_protocol",
  "graphics",
  "graphics_packages",
  "desktop_version",
  "env.XDG_CURRENT_DESKTOP",
  "env.XDG_SESSION_DESKTOP",
  "env.XDG_SESSION_TYPE",
  "env.DESKTOP_SESSION",
  "env.WEBKIT_DISABLE_DMABUF_RENDERER",
  "env.WEBKIT_DISABLE_COMPOSITING_MODE",
  "env.LIBGL_ALWAYS_SOFTWARE",
  "env.__NV_DISABLE_EXPLICIT_SYNC",
  "env.GDK_BACKEND",
  "env.GDK_GL",
];

#[derive(Args)]
pub(crate) struct DiagnosticsArgs {
  /// Print the report as JSON instead of text
  #[arg(long)]
  json: bool,
  /// Save the report to a new file instead of stdout (existing files are preserved)
  #[arg(long, value_name = "PATH")]
  output: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProbeStatus {
  Ok,
  Unset,
  Unavailable,
  Failed,
  TimedOut,
  NotApplicable,
}

#[derive(Debug, Serialize)]
struct DiagnosticField {
  status: ProbeStatus,
  value: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiagnosticsReport {
  schema_version: u32,
  fields: BTreeMap<String, DiagnosticField>,
}

impl DiagnosticsReport {
  fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
    ensure!(
      bytes.len() <= MAX_REPORT_BYTES,
      "Diagnostics report exceeds its size limit"
    );
    let mut fields = BTreeMap::new();
    let mut schema_version = None;
    for raw_line in bytes.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
      let line = String::from_utf8_lossy(raw_line);
      let (name, rest) = line.split_once(": ").context("Invalid diagnostics record")?;
      let (status, value) = rest.split_once(": ").context("Invalid diagnostics status")?;
      let raw_value_length = raw_line
        .len()
        .checked_sub(name.len() + status.len() + 4)
        .context("Invalid diagnostics framing")?;
      ensure!(raw_value_length <= 512, "Diagnostics field exceeds its size limit");
      ensure!(
        !value.chars().any(|c| c.is_ascii_control()),
        "Invalid diagnostics value"
      );
      let status = match status {
        "ok" => ProbeStatus::Ok,
        "unset" => ProbeStatus::Unset,
        "unavailable" => ProbeStatus::Unavailable,
        "failed" => ProbeStatus::Failed,
        "timed_out" => ProbeStatus::TimedOut,
        "not_applicable" => ProbeStatus::NotApplicable,
        _ => anyhow::bail!("Unknown diagnostics status"),
      };
      if name == "schema_version" {
        ensure!(schema_version.is_none(), "Duplicate diagnostics schema version");
        ensure!(
          matches!(status, ProbeStatus::Ok) && value == "1",
          "Unsupported diagnostics schema"
        );
        schema_version = Some(1);
        continue;
      }
      ensure!(FIELDS.contains(&name), "Unknown diagnostics field");
      let value = if matches!(status, ProbeStatus::Ok) {
        let mut normalized = String::new();
        for character in value.chars() {
          let character = if character.is_control() { ' ' } else { character };
          if normalized.len() + character.len_utf8() > 512 {
            break;
          }
          normalized.push(character);
        }
        Some(normalized)
      } else {
        ensure!(value.is_empty(), "Unexpected value for unavailable diagnostics field");
        None
      };
      ensure!(
        fields
          .insert(name.to_owned(), DiagnosticField { status, value })
          .is_none(),
        "Duplicate diagnostics field"
      );
    }
    ensure!(fields.len() == FIELDS.len(), "Incomplete diagnostics report");
    Ok(Self {
      schema_version: schema_version.context("Missing diagnostics schema version")?,
      fields,
    })
  }

  fn text(&self) -> String {
    let mut text = format!("schema_version: ok: {}\n", self.schema_version);
    for name in FIELDS {
      let field = &self.fields[*name];
      let status = match field.status {
        ProbeStatus::Ok => "ok",
        ProbeStatus::Unset => "unset",
        ProbeStatus::Unavailable => "unavailable",
        ProbeStatus::Failed => "failed",
        ProbeStatus::TimedOut => "timed_out",
        ProbeStatus::NotApplicable => "not_applicable",
      };
      text.push_str(&format!(
        "{name}: {status}: {}\n",
        field.value.as_deref().unwrap_or_default()
      ));
    }
    text
  }
}

impl DiagnosticsArgs {
  pub(crate) async fn run(&self, version: &str) -> anyhow::Result<()> {
    let mut child = Command::new("/bin/sh")
      .args(["-s", "--", "--client-version", version, "--client-path"])
      .arg(std::env::current_exe().context("Cannot locate gpclient")?)
      .arg("--auth-path")
      .arg(binary_paths::gpauth())
      .arg("--gui-path")
      .arg(binary_paths::gpgui())
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::inherit())
      .kill_on_drop(true)
      .spawn()
      .context("Cannot start diagnostics collector")?;
    let mut input = child.stdin.take().context("Collector input is unavailable")?;
    let (_, output) = tokio::try_join!(
      async move {
        input.write_all(COLLECTOR.as_bytes()).await?;
        drop(input);
        Ok::<_, std::io::Error>(())
      },
      child.wait_with_output(),
    )
    .context("Diagnostics collection failed")?;
    ensure!(
      output.status.success(),
      "Diagnostics collector exited with {}",
      output.status
    );
    let report = DiagnosticsReport::parse(&output.stdout)?;
    let content = if self.json {
      format!("{}\n", serde_json::to_string_pretty(&report)?)
    } else {
      report.text()
    };
    if let Some(path) = &self.output {
      save_report(path, &content)?;
    } else {
      std::io::stdout().lock().write_all(content.as_bytes())?;
    }
    Ok(())
  }
}

fn save_report(path: &std::path::Path, content: &str) -> anyhow::Result<()> {
  let mut file = OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(path)
    .context("Cannot create diagnostics file; use a new path")?;
  file
    .write_all(content.as_bytes())
    .context("Cannot save diagnostics report")
}

#[cfg(test)]
mod tests {
  use super::*;

  fn sample_report() -> String {
    let mut text = String::from("schema_version: ok: 1\n");
    for name in FIELDS {
      text.push_str(&format!("{name}: unavailable: \n"));
    }
    text
  }

  #[test]
  fn json_preserves_collected_values_and_distinguishes_missing_probes() {
    let text = sample_report().replace("os: unavailable: ", "os: ok: FreeBSD");
    let report = DiagnosticsReport::parse(text.as_bytes()).unwrap();
    let json = serde_json::to_value(report).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["fields"]["os"]["value"], "FreeBSD");
    assert_eq!(json["fields"]["graphics"]["status"], "unavailable");
    assert!(json["fields"]["graphics"]["value"].is_null());
  }

  #[test]
  fn malformed_or_injected_records_are_rejected() {
    let good = sample_report();
    for bad in [
      good.replace("os: unavailable: ", "os: mystery: "),
      good.replace("os: unavailable: ", "os: ok: text\u{1b}[31m"),
      good.replace("os: unavailable: ", "os: ok: value\nsecret: ok: token"),
      format!("{good}os: ok: duplicate\n"),
      good.replace("schema_version: ok: 1", "schema_version: ok: 2"),
      good.replace("os: unavailable: \n", ""),
    ] {
      assert!(DiagnosticsReport::parse(bad.as_bytes()).is_err());
    }
  }

  #[test]
  fn invalid_utf8_and_unicode_controls_produce_a_bounded_readable_value() {
    let mut bytes = sample_report()
      .replace("os: unavailable: ", "os: ok: REPLACE")
      .into_bytes();
    let start = bytes.windows(7).position(|value| value == b"REPLACE").unwrap();
    bytes.splice(start..start + 7, [0xff; 500]);
    let report = DiagnosticsReport::parse(&bytes).unwrap();
    let value = report.fields["os"].value.as_ref().unwrap();
    assert!(value.len() <= 512);
    assert!(value.starts_with('\u{fffd}'));
    let text = sample_report().replace("os: unavailable: ", "os: ok: Test\u{85}OS");
    let report = DiagnosticsReport::parse(text.as_bytes()).unwrap();
    assert_eq!(report.fields["os"].value.as_deref(), Some("Test OS"));
    assert!(report.text().contains("os: ok: Test OS"));
  }

  #[test]
  fn saving_a_report_does_not_overwrite_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("diagnostics.txt");
    save_report(&path, "first").unwrap();
    assert!(save_report(&path, "second").is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "first");
  }
}
