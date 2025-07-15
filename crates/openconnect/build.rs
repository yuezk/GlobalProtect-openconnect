fn main() {
  // Link to the native openconnect library
  println!("cargo:rustc-link-lib=openconnect");

  // Determine library and include paths based on environment
  let (lib_path, include_path) = if let Ok(conda_prefix) = std::env::var("CONDA_PREFIX") {
    // Use conda environment paths
    (format!("{}/lib", conda_prefix), format!("{}/include", conda_prefix))
  } else if std::path::Path::new("/opt/homebrew/lib").exists() {
    // Fallback to Homebrew on macOS
    ("/opt/homebrew/lib".to_string(), "/opt/homebrew/include".to_string())
  } else {
    // System default paths
    ("/usr/lib".to_string(), "/usr/include".to_string())
  };

  println!("cargo:rustc-link-search={}", lib_path);

  // Explicitly link all OpenConnect dependencies
  println!("cargo:rustc-link-lib=gnutls");
  println!("cargo:rustc-link-lib=hogweed");
  println!("cargo:rustc-link-lib=gmp");
  println!("cargo:rustc-link-lib=xml2");
  println!("cargo:rustc-link-lib=p11-kit");
  println!("cargo:rustc-link-lib=stoken");
  println!("cargo:rustc-link-lib=gssapi_krb5");
  println!("cargo:rustc-link-lib=iconv");
  println!("cargo:rustc-link-lib=pcsclite");
  println!("cargo:rustc-link-lib=lz4");

  println!("cargo:rerun-if-changed=src/ffi/vpn.c");
  println!("cargo:rerun-if-changed=src/ffi/vpn.h");

  // Compile the vpn.c file
  cc::Build::new()
    .file("src/ffi/vpn.c")
    .include("src/ffi")
    .include(&include_path)
    .compile("vpn");
}
