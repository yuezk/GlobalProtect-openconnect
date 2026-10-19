use std::{fs, path::PathBuf, process::Command};

#[test]
fn c_network_configuration_lifecycle_and_failure_paths() {
  let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let build = PathBuf::from(env!("OPENCONNECT_PROTOCOL_BUILD"));
  let source = PathBuf::from(env!("OPENCONNECT_PROTOCOL_SOURCE"));
  let directory = std::env::temp_dir().join(format!("gp-network-configuration-{}", std::process::id()));
  fs::create_dir_all(&directory).unwrap();
  let executable = directory.join("network-configuration");
  let flags = Command::new("pkg-config")
    .args(["--cflags", "libxml-2.0", "gnutls"])
    .output()
    .unwrap();
  assert!(flags.status.success());
  let output = Command::new(build.join("libtool"))
    .args(["--mode=link", "cc"])
    .arg(manifest.join("tests/network_configuration.c"))
    // Compile the production setup implementation with only OS device creation
    // replaced, so rollback is exercised without privileges or a real TUN.
    .arg(source.join("library.c"))
    .arg("-Dos_setup_tun=test_os_setup_tun")
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
    .output()
    .unwrap();
  assert!(
    output.status.success(),
    "{}: {}",
    output.status,
    String::from_utf8_lossy(&output.stderr)
  );
  let output = Command::new(&executable).output().unwrap();
  fs::remove_dir_all(&directory).unwrap();
  assert!(
    output.status.success(),
    "{}: {}",
    output.status,
    String::from_utf8_lossy(&output.stderr)
  );
}
