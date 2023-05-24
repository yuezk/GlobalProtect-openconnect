use gpcommon::sha256_digest;
use std::path::Path;
use std::{env, fs};

fn main() {
    let gpservice_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let profile = env::var("PROFILE").unwrap();
    let gpclient_path = Path::new(&gpservice_dir)
        .join("../target")
        .join(profile)
        .join("gpclient");

    if !gpclient_path.exists() {
        // error if gpclient doesn't exist
        panic!("Please build gpclient first");
    }

    if let Ok(digest) = sha256_digest(gpclient_path) {
        let out_dir = env::var("OUT_DIR").unwrap();
        let dest_path = format!("{out_dir}/client_hash.rs");
        fs::write(dest_path, format!("pub const GPCLIENT_HASH: &str = \"{digest}\";")).unwrap();
    } else {
        panic!("Error: Unable to get gpclient hash");
    }
}
