#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use env_logger::Env;
use gpcommon::{Client, VpnStatus};
use log::warn;
use serde::Serialize;
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_log::LogTarget;

mod auth;
mod commands;

#[derive(Debug, Clone, Serialize)]
struct StatusPayload {
    status: VpnStatus,
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(Client::default());
    let client_clone = client.clone();
    let app_handle = app.handle();

    tauri::async_runtime::spawn(async move {
        let _ = client_clone.subscribe_status(move |status| {
            let payload = StatusPayload { status };
            if let Err(err) = app_handle.emit_all("vpn-status-received", payload) {
                warn!("Error emitting event: {}", err);
            }
        });

        let _ = client_clone.run().await;
    });

    app.manage(client);
    Ok(())
}

fn main() {
    // env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .targets([
                    LogTarget::LogDir,
                    LogTarget::Stdout, /*LogTarget::Webview*/
                ])
                .level(log::LevelFilter::Info)
                .build(),
        )
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            commands::vpn_status,
            commands::vpn_connect,
            commands::vpn_disconnect,
            commands::saml_login
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
