//! Link the MinGW-built OpenConnect DLL into the native MSVC Rust service.
use std::{env, path::PathBuf};

pub fn build() {
  println!("cargo:rerun-if-env-changed=OPENCONNECT_WINDOWS_PREFIX");
  let prefix = PathBuf::from(
    env::var_os("OPENCONNECT_WINDOWS_PREFIX")
      .expect("Build native OpenConnect and set OPENCONNECT_WINDOWS_PREFIX to its install directory"),
  );
  let include = prefix.join("include");
  let library = prefix.join("lib");
  assert!(
    library.join("openconnect.lib").is_file(),
    "The native OpenConnect MSVC import library is missing"
  );
  println!("cargo:rerun-if-changed={}", library.join("openconnect.lib").display());
  println!("cargo:rerun-if-changed={}", include.join("openconnect.h").display());
  println!("cargo:rustc-link-search=native={}", library.display());
  println!("cargo:rustc-link-lib=dylib=openconnect");
  println!("cargo:rustc-link-lib=ws2_32");
  println!("cargo:include={}", include.display());
  println!("cargo:lib_dir={}", prefix.join("bin").display());
  println!("cargo:rerun-if-changed=src/ffi/vpn.c");
  println!("cargo:rerun-if-changed=src/ffi/vpn.h");
  cc::Build::new()
    .file("src/ffi/vpn.c")
    .include("src/ffi")
    .include(include)
    .std("c11")
    .compile("vpn");
}
