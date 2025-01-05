use std::path::Path;

use log::{info, warn};
use regex::Regex;
use tempfile::NamedTempFile;
use version_compare::{compare_to, Cmp};

pub fn openssl_conf() -> String {
  let option = get_openssl_option();

  format!(
    "openssl_conf = openssl_init

[openssl_init]
ssl_conf = ssl_sect
providers = provider_sect

[ssl_sect]
system_default = system_default_sect

[system_default_sect]
Options = {}

[provider_sect]
default = default_sect
legacy = legacy_sect

[default_sect]
activate = 1

[legacy_sect]
activate = 1
",
    option
  )
}

pub fn fix_openssl<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
  let content = openssl_conf();
  std::fs::write(path, content)?;
  Ok(())
}

pub fn fix_openssl_env() -> anyhow::Result<NamedTempFile> {
  let openssl_conf = NamedTempFile::new()?;
  let openssl_conf_path = openssl_conf.path();

  fix_openssl(openssl_conf_path)?;
  std::env::set_var("OPENSSL_CONF", openssl_conf_path);

  Ok(openssl_conf)
}

// See: https://stackoverflow.com/questions/75763525/curl-35-error0a000152ssl-routinesunsafe-legacy-renegotiation-disabled
fn get_openssl_option() -> &'static str {
  let version_str = openssl::version::version();
  let default_option = "UnsafeLegacyServerConnect";

  let Some(version) = extract_openssl_version(version_str) else {
    warn!("Failed to extract OpenSSL version from '{}'", version_str);
    return default_option;
  };

  let older_than_3_0_4 = match compare_to(version, "3.0.4", Cmp::Lt) {
    Ok(result) => result,
    Err(_) => {
      warn!("Failed to compare OpenSSL version: {}", version);
      return default_option;
    }
  };

  if older_than_3_0_4 {
    info!("Using 'UnsafeLegacyRenegotiation' option");
    "UnsafeLegacyRenegotiation"
  } else {
    info!("Using 'UnsafeLegacyServerConnect' option");
    default_option
  }
}

fn extract_openssl_version(version: &str) -> Option<&str> {
  let re = Regex::new(r"OpenSSL (\d+\.\d+\.\d+[^\s]*)").unwrap();
  re.captures(version).and_then(|caps| caps.get(1)).map(|m| m.as_str())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_extract_version() {
    let input = "OpenSSL 3.4.0 22 Oct 2024 (Library: OpenSSL 3.4.0 22 Oct 2024)";
    assert_eq!(extract_openssl_version(input), Some("3.4.0"));
  }

  #[test]
  fn test_different_format() {
    let input = "OpenSSL 1.1.1t  7 Feb 2023";
    assert_eq!(extract_openssl_version(input), Some("1.1.1t"));
  }

  #[test]
  fn test_invalid_input() {
    let input = "Invalid string without version";
    assert_eq!(extract_openssl_version(input), None);
  }
}
