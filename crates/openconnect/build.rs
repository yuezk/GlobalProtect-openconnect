use std::{env, path::PathBuf, process::Command};

fn main() {
  let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
  let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
  let deps_dir = PathBuf::from(&manifest_dir).join("../../deps");
  let openconnect_dir = deps_dir.join("openconnect");
  let gp_version_patch = deps_dir.join("patches/openconnect-gp-version.patch");

  // The temporary location where we will build
  let build_src = out_dir.join("openconnect_build");

  // Track changes so cargo rebuilds if patch or submodule changes
  println!("cargo:rerun-if-changed={}", openconnect_dir.display());
  println!("cargo:rerun-if-changed={}", gp_version_patch.display());

  // Prepare the build directory
  // If the directory exists from a previous run, remove it to ensure a clean slate
  if build_src.exists() {
    std::fs::remove_dir_all(&build_src).unwrap();
  }
  std::fs::create_dir_all(&build_src).unwrap();

  // Copy the source code to OUT_DIR
  // We use fs_extra options to copy contents recursively
  let mut options = fs_extra::dir::CopyOptions::new();
  options.overwrite = true;
  options.content_only = true; // Copy content of src into dest, not src folder itself

  fs_extra::dir::copy(&openconnect_dir, &build_src, &options).expect("Failed to copy C source code to OUT_DIR");

  // Apply the Patch
  // We use the system `patch` command.
  let status = Command::new("patch")
    .arg("-p1") // Strip one level of path (standard for git diffs)
    .arg("-i") // Input file
    .arg(&gp_version_patch)
    .current_dir(&build_src) // Run INSIDE the copied dir
    .status()
    .expect("Failed to execute patch command");

  if !status.success() {
    panic!("Failed to apply patch file: {}", gp_version_patch.display());
  }

  // Build the OpenConnect library using autotools
  // We explicitly enable static and disable shared to ensure we get a .a file.
  // .reconf("-ivf") is CRITICAL for git submodules because 'configure'
  // usually doesn't exist yet and needs to be generated.
  let dst = autotools::Config::new(&build_src)
    .reconf("-ivf")
    .enable_static()
    .disable_shared()
    .disable("nls", None) // disable translations to save space
    .disable("docs", None)
    .disable("dsa-tests", None)
    .without("libproxy", None)
    .without("stoken", None)
    .without("libpcsclite", None)
    .without("libpskc", None)
    .without("gssapi", None)
    .with("gnutls-tss2", Some("no"))
    .with("vpnc-script", Some("/etc/vpnc/vpnc-script")) // Specify vpnc-script, otherwise it will fail on macOS
    .build();

  // Link the 'openconnect' static library
  println!("cargo:rustc-link-search=native={}/lib", dst.display());
  println!("cargo:rustc-link-lib=static=openconnect");

  // Using pkg-config to find system deps
  pkg_config::probe_library("libxml-2.0").unwrap();
  pkg_config::probe_library("zlib").unwrap();
  pkg_config::probe_library("liblz4").unwrap();
  pkg_config::probe_library("gnutls").unwrap();

  // Below are required by gnutls
  pkg_config::probe_library("p11-kit-1").unwrap();
  pkg_config::probe_library("hogweed").unwrap();
  pkg_config::probe_library("nettle").unwrap();
  pkg_config::probe_library("gmp").unwrap();

  // Compile the vpn.c file
  println!("cargo:rerun-if-changed=src/ffi/vpn.c");
  println!("cargo:rerun-if-changed=src/ffi/vpn.h");

  cc::Build::new()
    .file("src/ffi/vpn.c")
    .include("src/ffi")
    .include(&dst.join("include"))
    .compile("vpn");
}
