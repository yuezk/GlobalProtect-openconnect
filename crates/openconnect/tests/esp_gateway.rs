use std::{fs, path::PathBuf, process::Command};

#[test]
fn c_esp_gateway_configuration_and_probes() {
  let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let build = PathBuf::from(env!("OPENCONNECT_PROTOCOL_BUILD"));
  let source = PathBuf::from(env!("OPENCONNECT_PROTOCOL_SOURCE"));
  let directory = tempfile::tempdir().unwrap();
  let executable = directory.path().join("esp-gateway");
  let mut packages = vec!["libxml-2.0", "gnutls"];
  if fs::read_to_string(build.join("config.h"))
    .unwrap()
    .contains("#define HAVE_LZ4 ")
  {
    packages.push("liblz4");
  }
  let flags = Command::new("pkg-config")
    .arg("--cflags")
    .args(packages)
    .output()
    .unwrap();
  assert!(flags.status.success(), "{}", String::from_utf8_lossy(&flags.stderr));
  let output = Command::new(build.join("libtool"))
    .args(["--mode=link", "cc"])
    .arg(manifest.join("tests/esp_gateway.c"))
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
  assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
  for case in [
    "regular-zero",
    "regular-address",
    "regular-invalid",
    "regular-missing",
    "nlb-zero",
    "nlb-address",
    "nlb-client-vip",
  ] {
    let output = Command::new(build.join("libtool"))
      .arg("--mode=execute")
      .arg(&executable)
      .arg(case)
      .output()
      .unwrap();
    assert!(
      output.status.success(),
      "{case}: {}",
      String::from_utf8_lossy(&output.stderr)
    );
  }
}
