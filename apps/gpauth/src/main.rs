#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;

#[cfg(all(target_os = "macos", feature = "webview-auth"))]
mod callback_app;

#[cfg(feature = "webview-auth")]
mod tauri_context;

#[cfg(feature = "webview-auth")]
mod webview_auth;

#[tokio::main]
async fn main() {
  #[cfg(all(target_os = "macos", feature = "webview-auth"))]
  if is_callback_app_launch() {
    if let Err(err) = callback_app::run() {
      eprintln!("Failed to handle authentication callback: {err}");
    }
    return;
  }

  cli::run().await;
}

#[cfg(all(target_os = "macos", feature = "webview-auth"))]
fn is_callback_app_launch() -> bool {
  std::env::args_os().len() == 1
    && std::env::current_exe().is_ok_and(|executable| {
      executable
        .ancestors()
        .any(|path| path.extension().is_some_and(|extension| extension == "app"))
    })
}
