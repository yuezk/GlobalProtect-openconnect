#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
use tauri_plugin_log::LogTarget;

mod auth;
mod commands;
mod crypto;
mod settings;
mod setup;
mod storage;
mod utils;

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .targets([LogTarget::LogDir, LogTarget::Stdout])
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(setup::setup)
        .invoke_handler(tauri::generate_handler![
            commands::service_online,
            commands::vpn_status,
            commands::vpn_connect,
            commands::vpn_disconnect,
            commands::saml_login,
            commands::os_version,
            commands::openssl_config,
            commands::update_openssl_config,
            commands::store_get,
            commands::store_set,
            commands::store_save,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
