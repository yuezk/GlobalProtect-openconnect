mod cli;
mod handlers;
mod routes;
mod vpn_task;
mod ws_connection;
mod ws_server;

#[tokio::main]
async fn main() {
  cli::run().await;
}
