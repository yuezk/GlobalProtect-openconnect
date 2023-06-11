use crate::auth::{self, AuthData, AuthRequest, SamlBinding, SamlLoginParams};
use gpcommon::{Client, ServerApiError, VpnStatus};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
pub(crate) async fn service_online<'a>(client: State<'a, Arc<Client>>) -> Result<bool, ()> {
    Ok(client.is_online().await)
}

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
    clear_cookies: bool,
    app_handle: AppHandle,
) -> tauri::Result<Option<AuthData>> {
    let user_agent = String::from("PAN GlobalProtect");
    let params = SamlLoginParams {
        auth_request: AuthRequest::new(binding, request),
        user_agent,
        clear_cookies,
        app_handle,
    };
    auth::saml_login(params).await
}
