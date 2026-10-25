#[path = "build/desktop.rs"]
mod desktop;
#[path = "build/ios.rs"]
mod ios;

fn main() {
  match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
    Ok("ios") => ios::build(),
    Ok("macos" | "linux" | "freebsd" | "openbsd") => desktop::build(),
    target => panic!("Unsupported OpenConnect target: {target:?}"),
  }
}
