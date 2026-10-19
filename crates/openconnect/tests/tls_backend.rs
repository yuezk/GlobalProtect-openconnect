use std::{
  fs,
  os::unix::process::CommandExt,
  path::{Path, PathBuf},
  process::{Child, Command},
  time::{Duration, Instant},
};

struct TestDirectory(PathBuf);
impl Drop for TestDirectory {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.0);
  }
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
  fn drop(&mut self) {
    unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL) };
    let _ = self.0.wait();
  }
}

fn run_bounded(command: &mut Command, log: &Path) {
  let output = fs::File::create(log).unwrap();
  let mut child = ChildGuard(
    command
      .process_group(0)
      .stdout(output.try_clone().unwrap())
      .stderr(output)
      .spawn()
      .unwrap(),
  );
  let deadline = Instant::now() + Duration::from_secs(45);
  loop {
    if let Some(status) = child.0.try_wait().unwrap() {
      assert!(status.success(), "{status}: {}", fs::read_to_string(log).unwrap());
      return;
    }
    assert!(
      Instant::now() < deadline,
      "TLS test timed out: {}",
      fs::read_to_string(log).unwrap()
    );
    std::thread::sleep(Duration::from_millis(20));
  }
}

#[test]
fn openconnect_certificate_policy_and_legacy_renegotiation() {
  let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let build = PathBuf::from(env!("OPENCONNECT_PROTOCOL_BUILD"));
  let source = PathBuf::from(env!("OPENCONNECT_PROTOCOL_SOURCE"));
  let directory = TestDirectory(std::env::temp_dir().join(format!("gp-tls-backend-{}", std::process::id())));
  fs::create_dir(&directory.0).unwrap();
  let executable = directory.0.join("tls-backend");
  let flags = Command::new("pkg-config")
    .args(["--cflags", "--libs", "libxml-2.0", "gnutls"])
    .output()
    .unwrap();
  assert!(flags.status.success(), "{}", String::from_utf8_lossy(&flags.stderr));
  run_bounded(
    Command::new(build.join("libtool"))
      .args(["--mode=link", "cc", "-pthread"])
      .arg(manifest.join("tests/tls_backend.c"))
      .arg("-I")
      .arg(&build)
      .arg("-I")
      .arg(&source)
      .arg("-I")
      .arg(source.join("json"))
      .arg(build.join("libopenconnect.la"))
      .args(String::from_utf8(flags.stdout).unwrap().split_whitespace())
      .arg("-o")
      .arg(&executable),
    &directory.0.join("compile.log"),
  );
  let certificates = manifest.join("deps/openconnect/tests/certs");
  run_bounded(
    Command::new(build.join("libtool"))
      .arg("--mode=execute")
      .arg(executable)
      .arg(certificates.join("server-cert.pem"))
      .arg(certificates.join("server-key.pem"))
      .arg(certificates.join("ca.pem"))
      .arg(certificates.join("user-cert.pem"))
      .arg(certificates.join("user-key-pkcs1.pem")),
    &directory.0.join("runtime.log"),
  );
}
