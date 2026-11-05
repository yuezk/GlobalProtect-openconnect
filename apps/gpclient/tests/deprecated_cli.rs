#![cfg(not(target_os = "macos"))]

use std::process::Command;

#[test]
fn background_worker_warns_once_for_the_deprecated_flag() {
  const WARNING: &str = "--fix-openssl is deprecated and has no effect. It will be removed in a future release.";
  let directory = tempfile::tempdir().unwrap();
  for deprecated in [false, true] {
    let log = directory.path().join(format!("background-{deprecated}.log"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_gpclient"));
    if deprecated {
      command.arg("--fix-openssl");
    }
    let output = command
      .arg("--lock-file")
      .arg(directory.path().join("client.pid"))
      .args([
        "--log-format",
        "json",
        "connect",
        "vpn.example.test",
        "--background",
        "--certificate",
      ])
      // Fail identity preparation before authentication or a network connection.
      .arg(directory.path().join("missing.pem"))
      .arg("--log-file")
      .arg(&log)
      .output()
      .unwrap();
    assert!(!output.status.success());
    let mut records = String::from_utf8(output.stderr).unwrap();
    records.push_str(&std::fs::read_to_string(log).unwrap());
    assert!(records.contains("Failed to read client identity"), "{records}");
    assert_eq!(records.matches(WARNING).count(), usize::from(deprecated), "{records}");
  }
}
