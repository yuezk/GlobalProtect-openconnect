use crate::storage::{AppStorage, KeyHint};
use serde::Deserialize;
use tauri::{AppHandle, Manager};

const STORAGE_KEY: &str = "SETTINGS_DATA";

#[derive(Debug, Deserialize)]
struct Settings {
    #[serde(rename = "customOpenSSL")]
    custom_openssl: bool,
}

pub(crate) fn is_custom_openssl_enabled(app_handle: &AppHandle) -> bool {
    let app_storage = app_handle.state::<AppStorage>();
    let hint = KeyHint::new(STORAGE_KEY, false);
    let settings = app_storage.get::<Settings>(hint);

    settings.map_or(false, |settings| settings.custom_openssl)
}
