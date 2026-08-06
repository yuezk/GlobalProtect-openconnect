use auth::{WebviewAuthenticator, apply_auth_theme};
use gpapi::{auth::AuthWindowTheme, gp_params::GpParams};
use log::info;
use tauri::RunEvent;
use tempfile::NamedTempFile;

use crate::cli::print_auth_result;

pub async fn authenticate(
  server: String,
  gp_params: GpParams,
  auth_request: String,
  clean: bool,
  window_title: Option<String>,
  window_theme: AuthWindowTheme,
  mut openssl_conf: Option<NamedTempFile>,
) -> anyhow::Result<()> {
  let auth_host_id = gp_params.os_profile().host_identity().host_id().to_string();

  tauri::Builder::default()
    .setup(move |app| {
      #[cfg(target_os = "macos")]
      app.set_activation_policy(tauri::ActivationPolicy::Accessory);

      apply_auth_theme(app.handle(), window_theme);
      let app_handle = app.handle().clone();

      tauri::async_runtime::spawn(async move {
        let authenticator = WebviewAuthenticator::new(&server, &gp_params)
          .with_auth_request(&auth_request)
          .with_webview_user_agent(gp_params.os_profile().webview_user_agent())
          .with_clean(clean)
          .with_window_title(window_title.as_deref().unwrap_or("GlobalProtect Login"))
          .with_window_theme(window_theme);

        let auth_result = authenticator.authenticate(&app_handle).await;
        print_auth_result(auth_result, Some(&auth_host_id));

        // Ensure the app exits after the authentication process
        app_handle.exit(0);
      });

      Ok(())
    })
    .build(crate::tauri_context::get())?
    .run(move |_app_handle, event| {
      if let RunEvent::Exit = event {
        if let Some(file) = openssl_conf.take() {
          if let Err(err) = file.close() {
            info!("Error closing OpenSSL config file: {}", err);
          }
        }
      }
    });

  Ok(())
}
