use std::{env, path::PathBuf};

pub fn build() {
  println!("cargo:rerun-if-env-changed=GP_IOS_NATIVE_PREFIX");
  let prefix = PathBuf::from(env::var_os("GP_IOS_NATIVE_PREFIX").expect(
    "Set GP_IOS_NATIVE_PREFIX to target-built OpenConnect/GnuTLS libraries with an aarch64-apple-ios target.txt receipt",
  ));
  let target = std::fs::read_to_string(prefix.join("target.txt")).expect("Missing iOS native target receipt");
  assert_eq!(
    target.trim(),
    env::var("TARGET").unwrap(),
    "Native library target mismatch"
  );
  for name in ["openconnect", "gnutls", "hogweed", "nettle", "gmp"] {
    let archive = prefix.join(format!("lib/lib{name}.a"));
    assert!(archive.is_file(), "Missing {}", archive.display());
    println!("cargo:rerun-if-changed={}", archive.display());
  }
  println!("cargo:rerun-if-changed=src/ffi/vpn.c");
  println!("cargo:rerun-if-changed=src/ffi/vpn.h");
  cc::Build::new()
    .file("src/ffi/vpn.c")
    .include("src/ffi")
    .include(prefix.join("include"))
    .flag("-fapplication-extension")
    .compile("vpn");
  println!("cargo:rustc-link-search=native={}", prefix.join("lib").display());
  for name in ["openconnect", "gnutls", "hogweed", "nettle", "gmp"] {
    println!("cargo:rustc-link-lib=static={name}");
  }
  for name in ["xml2", "z", "iconv", "resolv"] {
    println!("cargo:rustc-link-lib={name}");
  }
  println!("cargo:rustc-link-lib=framework=Security");
  println!("cargo:rustc-link-lib=framework=CoreFoundation");
}
