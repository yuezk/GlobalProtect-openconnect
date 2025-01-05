use auth::WebviewAuthenticator;
use gpapi::gp_params::GpParams;
use log::info;
use tauri::RunEvent;
use tempfile::NamedTempFile;

use crate::cli::print_auth_result;

pub async fn authenticate(
  server: String,
  gp_params: GpParams,
  auth_request: String,
  clean: bool,
  mut openssl_conf: Option<NamedTempFile>,
) -> anyhow::Result<()> {
  tauri::Builder::default()
    .setup(move |app| {
      let app_handle = app.handle().clone();

      tauri::async_runtime::spawn(async move {
        let authenticator = WebviewAuthenticator::new(&server, &gp_params)
          .with_auth_request(&auth_request)
          .with_clean(clean);

        let auth_result = authenticator.authenticate(&app_handle).await;
        print_auth_result(auth_result);

        // Ensure the app exits after the authentication process
        app_handle.exit(0);
      });

      Ok(())
    })
    .build(tauri::generate_context!())?
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
