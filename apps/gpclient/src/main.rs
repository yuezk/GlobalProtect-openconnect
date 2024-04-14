mod cli;
mod connect;
mod disconnect;
mod launch_gui;

pub(crate) const GP_CLIENT_LOCK_FILE: &str = "/var/run/gpclient.lock";
pub(crate) const GP_CLIENT_PORT_FILE: &str = "/var/run/gpclient.port";

#[tokio::main]
async fn main() {
  cli::run().await;
}
