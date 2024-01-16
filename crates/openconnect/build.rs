fn main() {
  // Link to the native openconnect library
  println!("cargo:rustc-link-lib=openconnect");
  println!("cargo:rerun-if-changed=src/ffi/vpn.c");
  println!("cargo:rerun-if-changed=src/ffi/vpn.h");

  // Compile the vpn.c file
  cc::Build::new()
    .file("src/ffi/vpn.c")
    .include("src/ffi")
    .compile("vpn");
}
