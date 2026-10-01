use std::{
  fs,
  io::{BufRead, BufReader},
  os::unix::process::CommandExt,
  path::PathBuf,
  process::{Child, Command, Stdio},
  sync::mpsc,
  time::{Duration, Instant},
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
  fn drop(&mut self) {
    unsafe {
      libc::kill(-(self.0.id() as i32), libc::SIGKILL);
    }
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}
struct TestDirectory(PathBuf);
impl Drop for TestDirectory {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.0);
  }
}

#[test]
fn c_protocol_submits_initial_periodic_and_reconnect_reports() {
  let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let build = PathBuf::from(env!("OPENCONNECT_PROTOCOL_BUILD"));
  let source = PathBuf::from(env!("OPENCONNECT_PROTOCOL_SOURCE"));
  let directory = TestDirectory(std::env::temp_dir().join(format!("gp-hip-protocol-{}", std::process::id())));
  fs::create_dir(&directory.0).unwrap();
  let executable = directory.0.join("hip-protocol");
  let flags = Command::new("pkg-config")
    .args([
      "--cflags",
      "libxml-2.0",
      if cfg!(target_os = "macos") { "openssl" } else { "gnutls" },
    ])
    .output()
    .unwrap();
  assert!(flags.status.success());
  let compile_command = Command::new(build.join("libtool"))
    .args(["--mode=link", "cc"])
    .arg(manifest.join("tests/hip_protocol.c"))
    .arg("-I")
    .arg(&build)
    .arg("-I")
    .arg(&source)
    .arg("-I")
    .arg(source.join("json"))
    .args(String::from_utf8(flags.stdout).unwrap().split_whitespace())
    .arg(build.join("libopenconnect.la"))
    .arg("-o")
    .arg(&executable)
    .process_group(0)
    .stdout(Stdio::from(fs::File::create(directory.0.join("compile.log")).unwrap()))
    .stderr(Stdio::from(
      fs::File::options()
        .append(true)
        .open(directory.0.join("compile.log"))
        .unwrap(),
    ))
    .spawn()
    .unwrap();
  let mut compile_command = ChildGuard(compile_command);
  wait_bounded(
    &mut compile_command,
    Duration::from_secs(30),
    &directory.0.join("compile.log"),
  );
  let records = directory.0.join("submissions");
  let certificates = manifest.join("deps/openconnect/tests/certs");
  let server = Command::new("python3")
    .arg(manifest.join("tests/hip_gateway.py"))
    .arg(certificates.join("server-cert.pem"))
    .arg(certificates.join("server-key.pem"))
    .arg(&records)
    .process_group(0)
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .unwrap();
  let mut server = ChildGuard(server);
  let stdout = server.0.stdout.take().unwrap();
  let (tx, rx) = mpsc::channel();
  let reader = std::thread::spawn(move || {
    let mut port = String::new();
    let result = BufReader::new(stdout).read_line(&mut port).map(|_| port);
    let _ = tx.send(result);
  });
  let port = match rx.recv_timeout(Duration::from_secs(3)) {
    Ok(result) => result.expect("Fake gateway startup failed"),
    Err(error) => {
      let _ = server.0.kill();
      let _ = server.0.wait();
      let _ = reader.join();
      panic!("Fake gateway startup timed out: {error}");
    }
  };
  reader.join().unwrap();
  let port: u16 = port.trim().parse().expect("Fake gateway did not start");
  let log = fs::File::create(directory.0.join("client.log")).unwrap();
  let mut client = ChildGuard(
    Command::new(build.join("libtool"))
      .args(["--mode=execute"])
      .arg(executable)
      .arg(format!("https://127.0.0.1:{port}"))
      .process_group(0)
      .stdout(log.try_clone().unwrap())
      .stderr(log)
      .spawn()
      .unwrap(),
  );
  wait_bounded(&mut client, Duration::from_secs(15), &directory.0.join("client.log"));
  let reports = fs::read_to_string(records).unwrap();
  assert_eq!(
    reports,
    "<hip seq=\"1\" ip=\"10.0.0.1\"/>\n<hip seq=\"2\" ip=\"10.0.0.1\"/>\n<hip seq=\"3\" ip=\"10.0.0.1\"/>\n<hip seq=\"4\" ip=\"10.0.0.1\"/>\n"
  );
}

fn wait_bounded(child: &mut ChildGuard, timeout: Duration, log: &std::path::Path) {
  let deadline = Instant::now() + timeout;
  loop {
    if let Some(status) = child.0.try_wait().unwrap() {
      assert!(status.success(), "{}", fs::read_to_string(log).unwrap());
      return;
    }
    assert!(
      Instant::now() < deadline,
      "HIP protocol test process timed out: {}",
      fs::read_to_string(log).unwrap()
    );
    std::thread::sleep(Duration::from_millis(20));
  }
}
