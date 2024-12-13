#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;

#[tokio::main]
async fn main() {
  cli::run().await;
}
