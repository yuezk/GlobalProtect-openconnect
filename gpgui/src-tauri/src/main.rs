#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use common::{Client, ServerApiError, VpnStatus};
use serde::Serialize;
use std::sync::Arc;
use tauri::{Manager, State};

#[tauri::command]
async fn vpn_status<'a>(client: State<'a, Arc<Client>>) -> Result<VpnStatus, ServerApiError> {
    client.status().await
}

#[tauri::command]
async fn vpn_connect<'a>(
    server: String,
    cookie: String,
    client: State<'a, Arc<Client>>,
) -> Result<(), ServerApiError> {
    client.connect(server, cookie).await
}

#[tauri::command]
async fn vpn_disconnect<'a>(client: State<'a, Arc<Client>>) -> Result<(), ServerApiError> {
    client.disconnect().await
}

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
                println!("Error emmiting event: {}", err);
            }
        });

        let _ = client_clone.run().await;
    });

    app.manage(client);
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            vpn_status,
            vpn_connect,
            vpn_disconnect
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}