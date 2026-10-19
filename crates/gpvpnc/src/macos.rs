use std::{
  io::{self, BufRead, BufReader, Read},
  process::{Command, Stdio},
  thread,
};

const VPNC_SCRIPT: &str = include_str!("macos-vpnc-script.sh");

pub fn configure_network(reason: &str, environment: &[(String, Option<String>)]) -> io::Result<()> {
  run_script(VPNC_SCRIPT, reason, environment)
}

fn run_script(script: &str, reason: &str, environment: &[(String, Option<String>)]) -> io::Result<()> {
  // Only compiled-in content reaches the shell. Gateway values stay in a
  // child-local environment, never interpolated into shell commands.
  let mut command = Command::new("/bin/sh");
  command.args(["-c", script, "vpnc-script"]).env_clear().current_dir("/");
  for (name, value) in environment {
    match value {
      Some(value) => {
        command.env(name, value);
      }
      None => {
        command.env_remove(name);
      }
    }
  }
  command
    .env("reason", reason)
    .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
    .env("HOME", "/var/root")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  let mut child = command.spawn()?;
  let stdout = child.stdout.take().expect("stdout is piped");
  let stderr = child.stderr.take().expect("stderr is piped");
  let status = thread::scope(|scope| {
    // Drain both pipes while waiting, so either stream can exceed pipe capacity.
    let stdout = scope.spawn(|| log_output(stdout, reason, log::Level::Info));
    let stderr = scope.spawn(|| log_output(stderr, reason, log::Level::Warn));
    let status = child.wait();
    let stdout = stdout
      .join()
      .unwrap_or_else(|_| Err(io::Error::other("VPN script stdout reader panicked")));
    let stderr = stderr
      .join()
      .unwrap_or_else(|_| Err(io::Error::other("VPN script stderr reader panicked")));
    stdout?;
    stderr?;
    status
  })?;
  if status.success() {
    Ok(())
  } else {
    Err(io::Error::other(format!(
      "VPN configuration failed for {reason}: {status}"
    )))
  }
}

fn log_output(output: impl Read, reason: &str, level: log::Level) -> io::Result<()> {
  let mut reader = BufReader::new(output);
  let mut line = Vec::new();
  loop {
    line.clear();
    // Split oversized lines into bounded chunks, including output without a newline.
    if reader.by_ref().take(8192).read_until(b'\n', &mut line)? == 0 {
      return Ok(());
    }
    if line.last() == Some(&b'\n') {
      line.pop();
      if line.last() == Some(&b'\r') {
        line.pop();
      }
    }
    log::log!(level, "vpnc-script [{reason}]: {}", String::from_utf8_lossy(&line));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::Mutex;

  struct CapturedLogs(Mutex<Vec<(log::Level, String)>>);

  static LOGS: CapturedLogs = CapturedLogs(Mutex::new(Vec::new()));

  impl log::Log for CapturedLogs {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
      true
    }

    fn log(&self, record: &log::Record<'_>) {
      let message = record.args().to_string();
      if message.starts_with("vpnc-script [capture-test]: ") {
        self.0.lock().unwrap().push((record.level(), message));
      }
    }

    fn flush(&self) {}
  }

  #[test]
  fn captures_both_streams_with_levels_and_reason_even_on_failure() {
    log::set_logger(&LOGS).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let error = run_script(
      "printf 'first\\n\\nlast'; printf 'warning\\r\\nfinal' >&2; exit 23",
      "capture-test",
      &[],
    )
    .unwrap_err();
    assert!(error.to_string().contains("23"));
    let mut records = std::mem::take(&mut *LOGS.0.lock().unwrap());
    records.sort();
    let mut expected = vec![
      (log::Level::Info, "vpnc-script [capture-test]: first".to_string()),
      (log::Level::Info, "vpnc-script [capture-test]: ".to_string()),
      (log::Level::Info, "vpnc-script [capture-test]: last".to_string()),
      (log::Level::Warn, "vpnc-script [capture-test]: warning".to_string()),
      (log::Level::Warn, "vpnc-script [capture-test]: final".to_string()),
    ];
    expected.sort();
    assert_eq!(records, expected);

    // Each stream exceeds pipe capacity. Both must be drained before wait returns.
    run_script(
      r#"
      i=0
      while [ "$i" -lt 3000 ]; do
        printf 'stdout-abcdefghijklmnopqrstuvwxyz0123456789\n'
        printf 'stderr-abcdefghijklmnopqrstuvwxyz0123456789\n' >&2
        i=$((i + 1))
      done
      "#,
      "capture-test",
      &[],
    )
    .unwrap();
    let records = std::mem::take(&mut *LOGS.0.lock().unwrap());
    assert_eq!(
      records.iter().filter(|(level, _)| *level == log::Level::Info).count(),
      3000
    );
    assert_eq!(
      records.iter().filter(|(level, _)| *level == log::Level::Warn).count(),
      3000
    );

    log_output(&vec![b'x'; 20000][..], "capture-test", log::Level::Info).unwrap();
    log_output(&[0xff, b'\n'][..], "capture-test", log::Level::Warn).unwrap();
    let records = std::mem::take(&mut *LOGS.0.lock().unwrap());
    let prefix = "vpnc-script [capture-test]: ";
    assert_eq!(records.len(), 4);
    assert_eq!(records[0].1.len() - prefix.len(), 8192);
    assert_eq!(records[1].1.len() - prefix.len(), 8192);
    assert_eq!(records[2].1.len() - prefix.len(), 3616);
    assert_eq!(records[3], (log::Level::Warn, format!("{prefix}\u{fffd}")));
  }

  #[test]
  fn embedded_shell_syntax_is_valid_and_does_not_load_vpnc_hooks() {
    assert!(
      Command::new("/bin/sh")
        .args(["-n", "-c", VPNC_SCRIPT, "vpnc-script"])
        .status()
        .unwrap()
        .success()
    );
    assert!(!VPNC_SCRIPT.contains("/etc/vpnc"));
    assert!(!VPNC_SCRIPT.contains("run_hooks"));
  }

  #[test]
  fn tunnel_self_routes_are_skipped_without_skipping_other_routes() {
    // Exercise the embedded route functions with a recorder instead of changing
    // the host's routing table or running the script's initialization.
    let mut script = String::from(
      "route() { printf '%s\\n' \"$*\"; }\n\
       INTERNAL_IP4_ADDRESS=10.197.121.133\n\
       route_syntax_netmask=-netmask\n\
       route_syntax_del=delete\n",
    );
    for name in ["set_network_route", "del_network_route"] {
      let declaration = format!("\t{name}() {{");
      let start = VPNC_SCRIPT.rfind(&declaration).unwrap();
      let end = start + VPNC_SCRIPT[start..].find("\n\t}").unwrap() + 3;
      script.push_str(&VPNC_SCRIPT[start..end]);
      script.push('\n');
    }
    script.push_str(
      "set_network_route 10.197.121.133 255.255.255.255 32 utun7\n\
       del_network_route 10.197.121.133 255.255.255.255 32 utun7\n\
       set_network_route 10.15.70.11 255.255.255.255 32 utun7\n\
       set_network_route 10.0.0.0 255.0.0.0 8 utun7\n\
       del_network_route 10.15.70.11 255.255.255.255 32 utun7\n\
       set_network_route 10.197.121.133 255.255.255.255 32 utun7 10.197.121.1\n",
    );
    let output = Command::new("/bin/sh").args(["-c", &script]).output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
      String::from_utf8(output.stdout).unwrap(),
      "add -net 10.15.70.11 -netmask 255.255.255.255 10.197.121.133\n\
       add -net 10.0.0.0 -netmask 255.0.0.0 10.197.121.133\n\
       delete -net 10.15.70.11 -netmask 255.255.255.255 10.197.121.133\n\
       add -net 10.197.121.133 -netmask 255.255.255.255 10.197.121.1\n"
    );
  }

  #[test]
  fn runner_passes_configuration_as_data_and_uses_a_trusted_environment() {
    let environment = vec![
      ("TUNDEV".into(), Some("utun3; exit 99".into())),
      ("UNSET_VARIABLE".into(), None),
      ("PATH".into(), Some("/untrusted".into())),
    ];
    run_script(
      r#"
      [ "$0" = vpnc-script ] && [ "$reason" = connect ] &&
      [ "$TUNDEV" = 'utun3; exit 99' ] && [ -z "${UNSET_VARIABLE+x}" ] &&
      [ "$PATH" = /usr/bin:/bin:/usr/sbin:/sbin ] && [ "$HOME" = /var/root ]
    "#,
      "connect",
      &environment,
    )
    .unwrap();
    assert!(run_script("exit 23", "connect", &environment).is_err());
  }
}
