// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpgui_helper::cli;

#[tokio::main]
async fn main() {
  cli::run()
}
