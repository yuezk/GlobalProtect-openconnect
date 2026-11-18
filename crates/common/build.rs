use std::path::Path;

fn main() {
  let manifest_dir = env!("CARGO_MANIFEST_DIR");
  let workspace_dir = Path::new(manifest_dir).ancestors().nth(2).unwrap();
  let gpgui_dir = workspace_dir.parent().unwrap().join("gpgui");

  let suffix = if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
    ".exe"
  } else {
    ""
  };
  let gp_service_binary = workspace_dir.join(format!("target/debug/gpservice{suffix}"));
  let gp_client_binary = workspace_dir.join(format!("target/debug/gpclient{suffix}"));
  let gp_auth_binary = workspace_dir.join(format!("target/debug/gpauth{suffix}"));
  let gp_gui_helper_binary = workspace_dir.join(format!("target/debug/gpgui-helper{suffix}"));
  let gp_gui_binary = gpgui_dir.join(format!("target/debug/gpgui{suffix}"));

  println!("cargo:rustc-env=GP_SERVICE_BINARY={}", gp_service_binary.display());
  println!("cargo:rustc-env=GP_CLIENT_BINARY={}", gp_client_binary.display());
  println!("cargo:rustc-env=GP_AUTH_BINARY={}", gp_auth_binary.display());
  println!(
    "cargo:rustc-env=GP_GUI_HELPER_BINARY={}",
    gp_gui_helper_binary.display()
  );
  println!("cargo:rustc-env=GP_GUI_BINARY={}", gp_gui_binary.display());
}
