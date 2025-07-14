mod cli;
mod connect;
mod disconnect;
mod launch_gui;

#[tokio::main]
async fn main() {
  cli::run().await;
}
