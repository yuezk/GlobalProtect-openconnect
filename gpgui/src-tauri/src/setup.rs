use crate::{settings, storage::AppStorage, utils::get_openssl_conf_path};
use gpcommon::{Client, ClientStatus, VpnStatus};
use log::{info, warn};
use serde::Serialize;
use std::sync::Arc;
use tauri::Manager;

#[derive(Debug, Clone, Serialize)]
struct VpnStatusPayload {
    status: VpnStatus,
}

fn setup_window(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")?;
    if desktop == "KDE" {
        if let Some(main_window) = app.get_window("main") {
            let _ = main_window.set_decorations(false);
        }
    }

    Ok(())
}

fn setup_app_storage(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.app_handle();
    let app_storage = AppStorage::new(app_handle);

    app.manage(app_storage);

    Ok(())
}

fn setup_app_env(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.app_handle();
    let use_custom_openssl = settings::is_custom_openssl_enabled(&app_handle);

    if use_custom_openssl {
        info!("Using custom OpenSSL config");

        let openssl_conf = get_openssl_conf_path(&app_handle).into_os_string();
        std::env::set_var("OPENSSL_CONF", openssl_conf);
    }

    Ok(())
}

fn setup_vpn_client(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle();
    let client = Arc::new(Client::default());
    let client_clone = client.clone();

    app.manage(client_clone);

    tauri::async_runtime::spawn(async move {
        client.subscribe_status(move |client_status| match client_status {
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
        let _ = client.run().await;
    });

    Ok(())
}

pub(crate) fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    setup_window(app)?;
    setup_app_storage(app)?;
    setup_app_env(app)?;
    setup_vpn_client(app)?;

    Ok(())
}
