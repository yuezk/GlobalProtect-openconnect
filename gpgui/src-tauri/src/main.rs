#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use crate::utils::get_openssl_conf_path;
use env_logger::Env;
use gpcommon::{Client, ClientStatus, VpnStatus};
use log::{info, warn};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc};
use tauri::{Manager, Wry};
use tauri_plugin_log::LogTarget;
use tauri_plugin_store::{with_store, StoreCollection};

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

    let stores = app.state::<StoreCollection<Wry>>();
    let path = PathBuf::from(".settings.dat");
    let _ = with_store(app_handle.clone(), stores, path, |store| {
        let settings_data = store.get("SETTINGS_DATA");
        let custom_openssl = settings_data.map_or(false, |data| {
            data["customOpenSSL"].as_bool().unwrap_or(false)
        });

        if custom_openssl {
            info!("Using custom OpenSSL config");
            let openssl_conf = get_openssl_conf_path(&app_handle).into_os_string();
            std::env::set_var("OPENSSL_CONF", openssl_conf);
        }
        Ok(())
    });

    tauri::async_runtime::spawn(async move {
        client_clone.subscribe_status(move |client_status| match client_status {
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

    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        if desktop == "KDE" {
            if let Some(main_window) = app.get_window("main") {
                let _ = main_window.set_decorations(false);
            }
        }
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
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            commands::service_online,
            commands::vpn_status,
            commands::vpn_connect,
            commands::vpn_disconnect,
            commands::saml_login,
            commands::os_version,
            commands::openssl_config,
            commands::update_openssl_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
