use std::{os::unix::fs::PermissionsExt, process::Stdio, time::Duration};

use tokio::process::Command;

fn executable(dir: &std::path::Path, name: &str, body: &str) {
  let path = dir.join(name);
  std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
  std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn embedded_collector_reports_missing_helpers_without_recursing_or_exposing_private_environment() {
  let dir = tempfile::tempdir().unwrap();
  let bin = dir.path().join("bin");
  std::fs::create_dir(&bin).unwrap();
  executable(
    &bin,
    "uname",
    "case \"$1\" in -s) echo Linux ;; -m) echo aarch64 ;; *) echo test-kernel ;; esac",
  );
  executable(&bin, "gpclient", "echo unexpected-recursion >&2; exit 99");
  executable(&bin, "systemd-detect-virt", "echo kvm");
  executable(
    &bin,
    "lspci",
    "echo '00:02.0 VGA compatible controller: Test GPU'; echo '    Kernel driver in use: test'",
  );
  executable(&bin, "pacman", "echo 'gtk3 test-version'");
  let report_file = dir.path().join("report.json");
  let output = tokio::time::timeout(
    Duration::from_secs(30),
    Command::new(env!("CARGO_BIN_EXE_gpclient"))
      .args([
        #[cfg(not(target_os = "macos"))]
        "--fix-openssl",
        "diagnostics",
        "--json",
        "--output",
      ])
      .arg(&report_file)
      .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
      .env("GP_AUTH_BINARY", dir.path().join("missing-gpauth"))
      .env("GP_GUI_BINARY", dir.path().join("missing-gpgui"))
      .env("GP_DIAGNOSTICS_PRIVATE_TOKEN", "secret-value-must-not-appear")
      .env("XDG_CURRENT_DESKTOP", "TestDesktop\nInjected: secret")
      .stdin(Stdio::null())
      .kill_on_drop(true)
      .output(),
  )
  .await
  .expect("collector must close stdin and terminate")
  .unwrap();
  assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
  assert!(output.stdout.is_empty());
  let text = std::fs::read_to_string(&report_file).unwrap();
  assert!(!text.contains("secret-value-must-not-appear"));
  assert!(!String::from_utf8_lossy(&output.stderr).contains("unexpected-recursion"));
  let report: serde_json::Value = serde_json::from_str(&text).unwrap();
  assert_eq!(report["schema_version"], 1);
  assert_eq!(report["fields"]["os"]["value"], "Linux");
  assert_eq!(report["fields"]["gpclient_path"]["status"], "ok");
  for field in ["gpauth_path", "gpauth_version", "gpgui_path", "gpgui_version"] {
    assert_eq!(report["fields"][field]["status"], "unavailable");
    assert!(report["fields"][field]["value"].is_null());
  }
  assert!(
    report["fields"]["gpclient_version"]["value"]
      .as_str()
      .unwrap()
      .starts_with(env!("CARGO_PKG_VERSION"))
  );
  assert_eq!(
    report["fields"]["env.XDG_CURRENT_DESKTOP"]["value"],
    "TestDesktop Injected: secret"
  );
  assert!(!String::from_utf8_lossy(&output.stderr).contains("Using 'UnsafeLegacyServerConnect'"));
}
