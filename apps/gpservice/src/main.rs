mod cli;
mod handlers;
mod routes;
mod vpn_task;
mod ws_server;
mod ws_connection;

#[tokio::main]
async fn main() {
  cli::run().await;
}
