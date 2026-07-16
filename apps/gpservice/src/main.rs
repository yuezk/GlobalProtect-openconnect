mod cli;
#[cfg(unix)]
mod credential_lease;
#[cfg(debug_assertions)]
mod dev_bootstrap;
mod handlers;
#[cfg(target_os = "macos")]
mod macos_broker;
mod request_dispatcher;
mod routes;
mod session_registry;
mod vpn_task;
mod ws_connection;
mod ws_server;

#[tokio::main]
async fn main() {
  cli::run().await;
}
