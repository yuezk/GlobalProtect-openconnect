#![cfg(not(target_os = "macos"))]

use std::process::Command;

#[test]
fn deprecated_flag_warns_without_changing_auth_output_or_configuration() {
  const WARNING: &str = "--fix-openssl is deprecated and has no effect. It will be removed in a future release.";
  let directory = tempfile::tempdir().unwrap();
  let configuration = directory.path().join("openssl.cnf");
  std::fs::write(&configuration, "existing user configuration").unwrap();
  for deprecated in [false, true] {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gpauth"));
    command.arg("portal.example.com");
    if deprecated {
      command.arg("--fix-openssl");
    }
    // Reject invalid options before authentication, without making a network request.
    let output = command
      .args(["--browser-listen", "127.0.0.1", "--log-format", "json"])
      .env("OPENSSL_CONF", &configuration)
      .output()
      .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let logs = String::from_utf8(output.stderr).unwrap();
    let records = logs
      .lines()
      .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
      .collect::<Vec<_>>();
    assert_eq!(
      records.iter().filter(|record| record["message"] == WARNING).count(),
      usize::from(deprecated)
    );
    assert!(logs.contains("requires '--browser manual'"));
    assert_eq!(
      std::fs::read_to_string(&configuration).unwrap(),
      "existing user configuration"
    );
  }
}
