mod cli;
#[cfg(debug_assertions)]
mod dev_bootstrap;
mod handlers;
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
