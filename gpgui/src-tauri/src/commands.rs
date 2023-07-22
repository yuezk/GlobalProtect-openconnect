use crate::{
    auth::{self, AuthData, AuthRequest, SamlBinding, SamlLoginParams},
    storage::{AppStorage, KeyHint},
    utils::get_openssl_conf,
    utils::get_openssl_conf_path,
};
use gpcommon::{Client, ServerApiError, VpnStatus};
use serde_json::Value;
use std::{process::Stdio, sync::Arc};
use tauri::{AppHandle, State};
use tokio::{fs, io::AsyncWriteExt, process::Command};

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
    user_agent: String,
    client: State<'a, Arc<Client>>,
) -> Result<(), ServerApiError> {
    client.connect(server, cookie, user_agent).await
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
    user_agent: String,
    clear_cookies: bool,
    app_handle: AppHandle,
) -> tauri::Result<Option<AuthData>> {
    let params = SamlLoginParams {
        auth_request: AuthRequest::new(binding, request),
        user_agent,
        clear_cookies,
        app_handle,
    };
    auth::saml_login(params).await
}

#[tauri::command]
pub(crate) fn os_version() -> String {
    whoami::distro()
}

#[tauri::command]
pub(crate) async fn openssl_config() -> Result<String, ()> {
    Ok(get_openssl_conf())
}

#[tauri::command]
pub(crate) async fn update_openssl_config(app_handle: AppHandle) -> tauri::Result<()> {
    let openssl_conf = get_openssl_conf();
    let openssl_conf_path = get_openssl_conf_path(&app_handle);

    fs::write(openssl_conf_path, openssl_conf).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn openconnect_config() -> tauri::Result<String> {
    let file = "/etc/gpservice/gp.conf";
    let content = fs::read_to_string(file).await?;
    Ok(content)
}

#[tauri::command]
pub(crate) async fn update_openconnect_config(content: String) -> tauri::Result<i32> {
    let file = "/etc/gpservice/gp.conf";
    let mut child = Command::new("pkexec")
        .arg("tee")
        .arg(file)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();

    tokio::spawn(async move {
        stdin.write_all(content.as_bytes()).await.unwrap();
        drop(stdin);
    });

    let exit_status = child.wait().await?;

    exit_status.code().ok_or_else(|| {
        tauri::Error::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Process exited without a code",
        ))
    })
}

#[tauri::command]
pub(crate) async fn store_get<'a>(
    hint: KeyHint<'_>,
    app_storage: State<'_, AppStorage<'_>>,
) -> Result<Option<Value>, ()> {
    Ok(app_storage.get(hint))
}

#[tauri::command]
pub(crate) fn store_set(
    hint: KeyHint,
    value: Value,
    app_storage: State<'_, AppStorage>,
) -> Result<(), tauri_plugin_store::Error> {
    app_storage.set(hint, &value)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn store_save(
    app_storage: State<'_, AppStorage>,
) -> Result<(), tauri_plugin_store::Error> {
    app_storage.save()?;
    Ok(())
}
