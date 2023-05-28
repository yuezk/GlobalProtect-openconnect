use crate::auth::{self, AuthData, AuthRequest, SamlBinding};
use gpcommon::{Client, ServerApiError, VpnStatus};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
pub(crate) async fn vpn_status<'a>(
    client: State<'a, Arc<Client>>,
) -> Result<VpnStatus, ServerApiError> {
    client.status().await
}

#[tauri::command]
pub(crate) async fn vpn_connect<'a>(
    server: String,
    cookie: String,
    client: State<'a, Arc<Client>>,
) -> Result<(), ServerApiError> {
    client.connect(server, cookie).await
}

#[tauri::command]
pub(crate) async fn vpn_disconnect<'a>(
    client: State<'a, Arc<Client>>,
) -> Result<(), ServerApiError> {
    client.disconnect().await
}

#[tauri::command]
pub(crate) async fn saml_login(
    binding: SamlBinding,
    request: String,
    app_handle: AppHandle,
) -> tauri::Result<Option<AuthData>> {
    let ua = "PAN GlobalProtect";
    let clear_cookies = false;
    auth::saml_login(
        AuthRequest::new(binding, request),
        ua,
        clear_cookies,
        &app_handle,
    )
    .await
}
