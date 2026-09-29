use std::{
  fs,
  os::unix::fs::{MetadataExt, PermissionsExt},
  path::Path,
  process::Command,
};

use super::SentinelInfo;

const SENTINELCTL: &str = "/Library/Sentinel/sentinel-agent.bundle/Contents/MacOS/sentinelctl";

pub(super) fn detect() -> Option<SentinelInfo> {
  if !trusted_cli(Path::new(SENTINELCTL)) {
    return None;
  }

  let status = run(&["status"])?;
  let status = parse_status(&status)?;
  let real_time_protection = if status.active {
    let scan_status = run(&["scan-on-write"])?;
    parse_yes_no(scan_status.trim().strip_prefix("scan-on-write:")?)?
  } else {
    false
  };
  let firewall_enabled = run(&["firewall", "enabled"]).as_deref().and_then(parse_yes_no);

  Some(SentinelInfo {
    version: status.version,
    real_time_protection,
    firewall_enabled,
  })
}

fn trusted_cli(path: &Path) -> bool {
  path.ancestors().take_while(|part| part.as_os_str() != "/").all(|part| {
    let Ok(metadata) = fs::symlink_metadata(part) else {
      return false;
    };
    if metadata.uid() != 0 || metadata.permissions().mode() & 0o022 != 0 || metadata.file_type().is_symlink() {
      return false;
    }
    if part == path {
      metadata.is_file()
    } else {
      metadata.is_dir()
    }
  })
}

fn run(args: &[&str]) -> Option<String> {
  let output = Command::new(SENTINELCTL)
    .args(args)
    .env("LC_ALL", "C")
    .env_remove("SENTINEL_OUTPUT_JSON")
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }
  String::from_utf8(output.stdout).ok()
}

struct AgentStatus {
  version: String,
  active: bool,
}

fn parse_status(output: &str) -> Option<AgentStatus> {
  let version = status_value(output, "Version:")?;
  if version.is_empty()
    || version.len() > 64
    || !version
      .chars()
      .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
  {
    return None;
  }

  let operational = parse_status_bool(output, "Agent Operational State:", "enabled", "disabled")?;
  let ready = parse_status_bool(output, "Ready:", "yes", "no")?;
  let protected = parse_status_bool(output, "Protection:", "enabled", "disabled")?;
  Some(AgentStatus {
    version: version.to_string(),
    active: operational && ready && protected,
  })
}

fn status_value<'a>(output: &'a str, label: &str) -> Option<&'a str> {
  output
    .lines()
    .find_map(|line| line.trim().strip_prefix(label).map(str::trim))
}

fn parse_status_bool(output: &str, label: &str, yes: &str, no: &str) -> Option<bool> {
  match status_value(output, label)? {
    value if value == yes => Some(true),
    value if value == no => Some(false),
    _ => None,
  }
}

fn parse_yes_no(output: &str) -> Option<bool> {
  match output.trim() {
    "yes" => Some(true),
    "no" => Some(false),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_running_agent_status() {
    let status = parse_status(
      "Agent\n   Version: 25.3.4.8365\n   Agent Operational State: enabled\n   Ready: yes\n   Protection: enabled\n",
    )
    .unwrap();
    assert_eq!(status.version, "25.3.4.8365");
    assert!(status.active);
  }

  #[test]
  fn rejects_unknown_agent_state() {
    assert!(parse_status("Version: 25.3.4.8365\nProtection: maybe\n").is_none());
    assert!(
      !parse_status("Version: 25.3.4.8365\nAgent Operational State: disabled\nReady: yes\nProtection: enabled\n")
        .unwrap()
        .active
    );
  }

  #[test]
  fn parses_scan_and_firewall_states() {
    assert_eq!(
      parse_yes_no("scan-on-write:\tyes\n".trim().strip_prefix("scan-on-write:").unwrap()),
      Some(true)
    );
    assert_eq!(parse_yes_no("no\n"), Some(false));
    assert_eq!(parse_yes_no("unknown\n"), None);
  }
}
