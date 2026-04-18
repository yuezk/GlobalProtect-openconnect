use std::{env, path::PathBuf, process::Command};

fn apply_patches(patches_dir: &PathBuf, build_src: &PathBuf) {
  let mut patches = std::fs::read_dir(patches_dir)
    .expect("Failed to read patches directory")
    .filter_map(|entry| entry.ok())
    .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("patch"))
    .map(|entry| entry.path())
    .collect::<Vec<_>>();

  patches.sort();

  for patch in patches {
    println!("cargo:rerun-if-changed={}", patch.display());

    let status = Command::new("patch")
      .arg("-p1")
      .arg("-i")
      .arg(&patch)
      .current_dir(build_src)
      .status()
      .expect("Failed to execute patch command");

    if !status.success() {
      panic!("Failed to apply patch file: {}", patch.display());
    }
  }
}

fn build_libxml2(deps_dir: &PathBuf, out_dir: &PathBuf) -> PathBuf {
  let libxml2_dir = deps_dir.join("libxml2");
  let build_src = out_dir.join("libxml2_build");

  println!("cargo:rerun-if-changed={}", libxml2_dir.display());

  if build_src.exists() {
    std::fs::remove_dir_all(&build_src).unwrap();
  }
  std::fs::create_dir_all(&build_src).unwrap();

  let mut options = fs_extra::dir::CopyOptions::new();
  options.overwrite = true;
  options.content_only = true;
  fs_extra::dir::copy(&libxml2_dir, &build_src, &options).expect("Failed to copy libxml2 sources to OUT_DIR");

  let dst = autotools::Config::new(&build_src)
    .reconf("-ivf")
    .enable_static()
    .disable_shared()
    .with("python", Some("no"))
    .with("icu", Some("no"))
    .with("http", Some("no"))
    .with("ftp", Some("no"))
    .with("catalog", Some("no"))
    .with("docbook", Some("no"))
    .with("legacy", Some("no"))
    .with("threads", Some("no"))
    .with("zlib", Some("no"))
    .with("lzma", Some("no"))
    .cflag("-fPIC")
    .build();

  let lib_dir = dst.join("lib");
  let include_dir = dst.join("include/libxml2");

  println!("cargo:rustc-link-search=native={}", lib_dir.display());
  println!("cargo:rustc-link-lib=static=xml2");
  println!("cargo:include={}", include_dir.display());

  dst
}

fn build_openconnect(deps_dir: &PathBuf, out_dir: &PathBuf) -> PathBuf {
  let openconnect_dir = deps_dir.join("openconnect");
  let patches_dir = deps_dir.join("patches");
  let build_src = out_dir.join("openconnect_build");

  println!("cargo:rerun-if-changed={}", openconnect_dir.display());

  if build_src.exists() {
    std::fs::remove_dir_all(&build_src).unwrap();
  }
  std::fs::create_dir_all(&build_src).unwrap();

  let mut options = fs_extra::dir::CopyOptions::new();
  options.overwrite = true;
  options.content_only = true;
  fs_extra::dir::copy(&openconnect_dir, &build_src, &options).expect("Failed to copy openconnect sources to OUT_DIR");

  apply_patches(&patches_dir, &build_src);

  let dst = autotools::Config::new(&build_src)
    .reconf("-ivf")
    .enable_static()
    .disable_shared()
    .disable("nls", None)
    .disable("docs", None)
    .disable("dsa-tests", None)
    .without("libproxy", None)
    .without("stoken", None)
    .without("libpcsclite", None)
    .without("libpskc", None)
    .without("gssapi", None)
    .with("gnutls-tss2", Some("no"))
    .with("vpnc-script", Some("/usr/libexec/gpclient/vpnc-script"))
    .build();

  let lib_dir = dst.join("lib");
  let include_dir = dst.join("include");

  println!("cargo:rustc-link-search=native={}", lib_dir.display());
  println!("cargo:rustc-link-lib=static=openconnect");
  println!("cargo:include={}", include_dir.display());

  dst
}

fn main() {
  let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
  let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
  let deps_dir = PathBuf::from(manifest_dir).join("deps");

  let oc_dst = build_openconnect(&deps_dir, &out_dir);

  if env::var("LIBXML2_STATIC").is_ok() {
    let _libxml2_dst = build_libxml2(&deps_dir, &out_dir);
  } else {
    pkg_config::probe_library("libxml-2.0").unwrap();
  }

  pkg_config::probe_library("zlib").unwrap();
  pkg_config::probe_library("liblz4").unwrap();
  pkg_config::probe_library("gnutls").unwrap();
  pkg_config::probe_library("p11-kit-1").unwrap();
  pkg_config::probe_library("hogweed").unwrap();
  pkg_config::probe_library("nettle").unwrap();
  if pkg_config::probe_library("gmp").is_err() {
    println!("cargo:warning=Falling back to direct gmp linking because pkg-config metadata is unavailable");
    println!("cargo:rustc-link-lib=gmp");
  }

  println!("cargo:rerun-if-changed=src/ffi/vpn.c");
  println!("cargo:rerun-if-changed=src/ffi/vpn.h");

  #[cfg(target_os = "linux")]
  {
    println!("cargo:rustc-link-arg=-Wl,-Bstatic");
    println!("cargo:rustc-link-arg=-Wl,-Bdynamic");
  }

  cc::Build::new()
    .file("src/ffi/vpn.c")
    .include("src/ffi")
    .include(&oc_dst.join("include"))
    .compile("vpn");
}
