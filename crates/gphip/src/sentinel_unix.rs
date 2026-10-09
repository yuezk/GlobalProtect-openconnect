use std::{
  fs, io,
  os::unix::fs::{MetadataExt, PermissionsExt},
  path::{Path, PathBuf},
};

use gpapi::process::collection::CollectorCommands;

use super::SentinelInfo;

const SENTINEL_DIR: &str = "/opt/sentinelone";

pub(super) fn detect(commands: &CollectorCommands<'_>) -> io::Result<Option<SentinelInfo>> {
  let Some(cli) = trusted_cli() else {
    return Ok(None);
  };
  let Some(version) = run(commands, &cli, &["version"])?.as_deref().and_then(parse_version) else {
    return Ok(None);
  };
  let Some(agent_enabled) = run(commands, &cli, &["control", "status"])?
    .as_deref()
    .and_then(|status| parse_enabled(status, "Agent state"))
  else {
    return Ok(None);
  };
  let real_time_protection = if agent_enabled {
    let Some(policy) = run(commands, &cli, &["policy", "status"])? else {
      return Ok(None);
    };
    let (Some(on_write), Some(on_execute)) = (
      parse_enabled(&policy, "On-Write:"),
      parse_enabled(&policy, "On-Execute:"),
    ) else {
      return Ok(None);
    };
    on_write || on_execute
  } else {
    false
  };
  Ok(Some(SentinelInfo {
    version,
    real_time_protection,
    firewall_enabled: None,
  }))
}

fn trusted_cli() -> Option<PathBuf> {
  let agent_uid = uzers::get_user_by_name("sentinelone")?.uid();
  let agent_dir = Path::new(SENTINEL_DIR);
  if !trusted_dir(Path::new("/opt"), 0) {
    return None;
  }

  let link = fs::symlink_metadata(agent_dir).ok()?;
  if link.file_type().is_symlink() && link.uid() != 0 {
    return None;
  }
  let resolved_dir = fs::canonicalize(agent_dir).ok()?;
  for ancestor in resolved_dir.ancestors().skip(1) {
    if !trusted_dir(ancestor, 0) {
      return None;
    }
  }
  if !(trusted_dir(&resolved_dir, 0) || trusted_dir(&resolved_dir, agent_uid))
    || !trusted_dir(&resolved_dir.join("bin"), 0)
  {
    return None;
  }

  let cli = resolved_dir.join("bin/sentinelctl");
  let metadata = fs::symlink_metadata(&cli).ok()?;
  if !metadata.is_file()
    || metadata.uid() != 0
    || metadata.permissions().mode() & 0o022 != 0
    || metadata.permissions().mode() & 0o111 == 0
  {
    return None;
  }
  Some(cli)
}

fn trusted_dir(path: &Path, owner: u32) -> bool {
  let Ok(metadata) = fs::symlink_metadata(path) else {
    return false;
  };
  metadata.is_dir() && metadata.uid() == owner && metadata.permissions().mode() & 0o022 == 0
}

fn run(commands: &CollectorCommands<'_>, cli: &Path, args: &[&str]) -> io::Result<Option<String>> {
  Ok(
    commands
      .run_path(cli, args)?
      .filter(|output| output.status.success())
      .and_then(|output| String::from_utf8(output.stdout).ok()),
  )
}

fn parse_version(output: &str) -> Option<String> {
  let version = value(output, "Agent version:")?;
  if version.is_empty()
    || version.len() > 64
    || !version
      .chars()
      .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
  {
    return None;
  }
  Some(version.to_string())
}

fn parse_enabled(output: &str, label: &str) -> Option<bool> {
  match value(output, label)? {
    "Enabled" => Some(true),
    "Disabled" => Some(false),
    _ => None,
  }
}

fn value<'a>(output: &'a str, label: &str) -> Option<&'a str> {
  output
    .lines()
    .find_map(|line| line.trim().strip_prefix(label).map(str::trim))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_observed_agent_and_policy_status() {
    assert_eq!(
      parse_version("Agent version: 25.2.2.14\nSentinelCTL version: 25.2.2.14\n"),
      Some("25.2.2.14".into())
    );
    assert_eq!(parse_enabled("Agent state      Enabled\n", "Agent state"), Some(true));
    assert_eq!(
      parse_enabled("On-Write: Enabled\nOn-Execute: Disabled\n", "On-Write:"),
      Some(true)
    );
    assert_eq!(
      parse_enabled("On-Write: Enabled\nOn-Execute: Disabled\n", "On-Execute:"),
      Some(false)
    );
  }

  #[test]
  fn rejects_unknown_or_invalid_status() {
    assert_eq!(parse_version("Agent version: 25.2.2.14\"/>\n"), None);
    assert_eq!(parse_enabled("Agent state      Unknown\n", "Agent state"), None);
    assert_eq!(parse_enabled("On-Write: Unknown\n", "On-Write:"), None);
  }
}
