#[cfg(unix)]
mod background;
mod cli;
mod connect;
#[cfg(unix)]
mod dashboard;
#[cfg(unix)]
mod diagnostics;
#[cfg(unix)]
mod disconnect;
mod hip;
#[cfg(unix)]
mod launch_gui;
mod session;

#[cfg(unix)]
pub(crate) const GP_CLIENT_LOCK_FILE: &str = "/var/run/gpclient.lock";

#[tokio::main]
async fn main() {
  cli::run().await;
}
