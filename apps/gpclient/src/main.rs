mod cli;
mod connect;
mod disconnect;
mod launch_gui;

// Use runtime::get_client_lock_path() instead of this constant
#[deprecated(note = "Use gpapi::utils::runtime::get_client_lock_path() instead for proper user-specific paths")]
pub(crate) const GP_CLIENT_LOCK_FILE: &str = "/var/run/gpclient.lock";

#[tokio::main]
async fn main() {
  cli::run().await;
}
