fn main() {
    // Link to the native openconnect library
    println!("cargo:rustc-link-lib=openconnect");
    println!("cargo:rerun-if-changed=src/vpn/vpn.c");
    println!("cargo:rerun-if-changed=src/vpn/vpn.h");

    // Compile the wrapper.c file
    cc::Build::new()
        .file("src/vpn/vpn.c")
        .include("src/vpn")
        .compile("vpn");
}
