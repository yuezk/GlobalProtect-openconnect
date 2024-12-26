#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
#[cfg(feature = "webview-auth")]
mod webview_auth;

#[tokio::main]
async fn main() {
  cli::run().await;
}
