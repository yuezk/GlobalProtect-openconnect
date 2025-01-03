fn main() {
  // Link to the native openconnect library
  println!("cargo:rustc-link-lib=openconnect");
  println!("cargo:rustc-link-search=/opt/homebrew/lib"); // Homebrew path
  println!("cargo:rerun-if-changed=src/ffi/vpn.c");
  println!("cargo:rerun-if-changed=src/ffi/vpn.h");

  // Compile the vpn.c file
  cc::Build::new()
    .file("src/ffi/vpn.c")
    .include("src/ffi")
    .include("/opt/homebrew/include") // Homebrew path
    .compile("vpn");
}
