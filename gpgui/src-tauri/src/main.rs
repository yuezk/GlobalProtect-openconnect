#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use env_logger::Env;
use gpcommon::{Client, ClientStatus, VpnStatus};
use log::warn;
use serde::Serialize;
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_log::LogTarget;

mod auth;
mod commands;
mod utils;

#[derive(Debug, Clone, Serialize)]
struct VpnStatusPayload {
    status: VpnStatus,
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(Client::default());
    let client_clone = client.clone();
    let app_handle = app.handle();

    tauri::async_runtime::spawn(async move {
        let _ = client_clone.subscribe_status(move |client_status| match client_status {
            ClientStatus::Vpn(vpn_status) => {
                let payload = VpnStatusPayload { status: vpn_status };
                if let Err(err) = app_handle.emit_all("vpn-status-received", payload) {
                    warn!("Error emitting event: {}", err);
                }
            }
            ClientStatus::Service(is_online) => {
                if let Err(err) = app_handle.emit_all("service-status-changed", is_online) {
                    warn!("Error emitting event: {}", err);
                }
            }
        });

        let _ = client_clone.run().await;
    });

    app.manage(client);

    match std::env::var("XDG_CURRENT_DESKTOP") {
        Ok(desktop) => {
            if desktop == "KDE" {
                if let Some(main_window) = app.get_window("main") {
                    let _ = main_window.set_decorations(false);
                }
            }
        }
        Err(_) => (),
    }

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
                .with_colors(Default::default())
                .build(),
        )
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            commands::service_online,
            commands::vpn_status,
            commands::vpn_connect,
            commands::vpn_disconnect,
            commands::saml_login
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
