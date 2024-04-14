#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth_window;
mod browser_authenticator;
mod cli;

#[tokio::main]
async fn main() {
  cli::run().await;
}
