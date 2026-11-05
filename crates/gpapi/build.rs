fn main() {
  println!("cargo:rerun-if-changed=src/utils/certificate/gnutls.c");
  let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
  if !matches!(target.as_str(), "macos" | "linux" | "freebsd" | "openbsd") {
    return;
  }
  let mut package = pkg_config::Config::new();
  package.atleast_version("3.6").cargo_metadata(false);
  let library = package
    .probe("gnutls")
    .expect("GnuTLS development files are required for desktop certificate import");
  let mut build = cc::Build::new();
  build.file("src/utils/certificate/gnutls.c");
  for include in library.include_paths {
    build.include(include);
  }
  build.compile("gp_certificate_import");
  // GNU linkers must see the shim before its GnuTLS dependency (--as-needed).
  package
    .cargo_metadata(true)
    .probe("gnutls")
    .expect("GnuTLS was probed above");
}
