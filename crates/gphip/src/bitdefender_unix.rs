use std::{fs, io, path::Path};

use gpapi::process::collection::{CollectorCommands, root_owned_path};
use serde::Deserialize;

use super::BitdefenderInfo;

const BITDEFENDER_DIR: &str = "/opt/bitdefender-security-tools";
const MAX_VERSION_LEN: usize = 64;

pub(super) fn detect(commands: &CollectorCommands<'_>) -> io::Result<Option<BitdefenderInfo>> {
  let dir = Path::new(BITDEFENDER_DIR);
  if uzers::get_effective_uid() == 0 {
    let cli = dir.join("bin/bduitool");
    let status = commands
      .run_executable(&cli, &["get", "ps"])?
      .filter(|output| output.status.success())
      .and_then(|output| String::from_utf8(output.stdout).ok())
      .as_deref()
      .and_then(parse_product_status);
    if status.is_some() {
      return Ok(status);
    }
  }

  let Ok(manifest) = dir.join("etc/manifest.json").canonicalize() else {
    return Ok(None);
  };
  if !manifest.is_file() || !root_owned_path(&manifest) {
    return Ok(None);
  }
  let Ok(contents) = fs::read_to_string(&manifest) else {
    return Ok(None);
  };
  Ok(parse_manifest(&contents).map(|version| BitdefenderInfo {
    version,
    engine_version: None,
    real_time_protection: None,
  }))
}

/// Parse `bduitool get ps`. All three fields must be present for a full entry.
fn parse_product_status(output: &str) -> Option<BitdefenderInfo> {
  let version = product_version(value(output, "Product version:")?)?;
  let engine_version = value(output, "Engines version:")
    .filter(|version| valid_version(version))?
    .to_string();
  let real_time_protection = match value(output, "Antimalware status:")? {
    "On" => true,
    "Off" => false,
    _ => return None,
  };
  Some(BitdefenderInfo {
    version,
    engine_version: Some(engine_version),
    real_time_protection: Some(real_time_protection),
  })
}

fn parse_manifest(contents: &str) -> Option<String> {
  #[derive(Deserialize)]
  struct Manifest {
    version: String,
  }
  let manifest: Manifest = serde_json::from_str(contents).ok()?;
  product_version(&manifest.version)
}

/// The official client reports only the first three components, e.g. `7.10.0`.
fn product_version(version: &str) -> Option<String> {
  if !valid_version(version) {
    return None;
  }
  let short = version.split('.').take(3).collect::<Vec<_>>().join(".");
  Some(short)
}

fn valid_version(version: &str) -> bool {
  !version.is_empty()
    && version.len() <= MAX_VERSION_LEN
    && version.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
    && version.split('.').all(|part| !part.is_empty())
}

fn value<'a>(output: &'a str, label: &str) -> Option<&'a str> {
  output.lines().find_map(|line| {
    let line = line.trim();
    let line = line.strip_prefix("- ").unwrap_or(line);
    line.strip_prefix(label).map(str::trim)
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  const OBSERVED: &str = "\nProduct version: 7.10.0.200346\nLast product update: 2026-11-21 at 14:42:06 GMT\n\
New product update available: no\nEngines version: 7.101851\nLast security content update: 2026-11-21 at 13:42:12 GMT\n\
Features:\n  - Antimalware status: On\n  - AntiExploit status: On\n  - On-Access YARA status: Off\n";

  #[test]
  fn parses_observed_product_status() {
    let info = parse_product_status(OBSERVED).expect("observed output should parse");
    assert_eq!(info.version, "7.10.0");
    assert_eq!(info.engine_version.as_deref(), Some("7.101851"));
    assert_eq!(info.real_time_protection, Some(true));
  }

  #[test]
  fn antimalware_off_reports_no_real_time_protection() {
    let output = OBSERVED.replace("Antimalware status: On", "Antimalware status: Off");
    assert_eq!(parse_product_status(&output).unwrap().real_time_protection, Some(false));
  }

  #[test]
  fn incomplete_product_status_is_rejected() {
    assert!(parse_product_status(&OBSERVED.replace("Engines version: 7.101851\n", "")).is_none());
    assert!(parse_product_status(&OBSERVED.replace("  - Antimalware status: On\n", "")).is_none());
    assert!(parse_product_status(&OBSERVED.replace("Antimalware status: On", "Antimalware status: Unknown")).is_none());
    assert!(
      parse_product_status(&OBSERVED.replace("Engines version: 7.101851", "Engines version: 7.101851\"/>")).is_none()
    );
  }

  #[test]
  fn truncates_product_version_to_three_components() {
    assert_eq!(product_version("7.10.0.200346").as_deref(), Some("7.10.0"));
    assert_eq!(product_version("7.10.0").as_deref(), Some("7.10.0"));
    assert_eq!(product_version("7.10").as_deref(), Some("7.10"));
  }

  #[test]
  fn rejects_invalid_versions() {
    assert_eq!(product_version(""), None);
    assert_eq!(product_version("7.10.0\"/>"), None);
    assert_eq!(product_version(&"1".repeat(65)), None);
    assert_eq!(product_version("7..10"), None);
  }

  #[test]
  fn parses_manifest_version() {
    assert_eq!(
      parse_manifest(r#"{"version":"7.10.0.200346","changeset":"e95aa9d9"}"#).as_deref(),
      Some("7.10.0")
    );
    assert_eq!(parse_manifest("not json"), None);
    assert_eq!(parse_manifest(r#"{"changeset":"e95aa9d9"}"#), None);
  }
}
