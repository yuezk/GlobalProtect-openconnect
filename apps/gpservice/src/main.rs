#[cfg_attr(windows, path = "cli/windows.rs")]
mod cli;
#[cfg(all(unix, any(target_os = "macos", debug_assertions)))]
mod credential_lease;
#[cfg(unix)]
mod desktop_client;
#[cfg(all(debug_assertions, unix))]
mod dev_bootstrap;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", all(test, unix)))]
mod device_anchor;
#[cfg(unix)]
mod gui_installation;
mod handlers;
mod hip_preview;
mod hip_source;
#[cfg(target_os = "macos")]
mod macos_broker;
mod request_dispatcher;
mod routes;
#[cfg(unix)]
mod runtime_user;
mod session_registry;
mod vpn_script;
mod vpn_task;
mod ws_connection;
mod ws_server;

#[cfg(windows)]
mod windows_service;

const VERSION: &str = concat!(
  env!("CARGO_PKG_VERSION"),
  " (",
  env!("GPSERVICE_GIT_COMMIT"),
  " ",
  compile_time::date_str!(),
  ")"
);

#[cfg(unix)]
#[tokio::main]
async fn main() {
  cli::run().await;
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
  match cli::run() {
    Ok(()) => std::process::ExitCode::SUCCESS,
    Err(error) => {
      eprintln!("{error:#}");
      std::process::ExitCode::FAILURE
    }
  }
}
