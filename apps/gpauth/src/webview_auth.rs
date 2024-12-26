use auth::{Authenticator, WebviewAuthenticator};
use log::info;
use tauri::RunEvent;
use tempfile::NamedTempFile;

use crate::cli::{print_auth_result, Cli};

pub fn authenticate(
  cli: &Cli,
  authenticator: Authenticator<'static>,
  mut openssl_conf: Option<NamedTempFile>,
) -> anyhow::Result<()> {
  let authenticator = authenticator.with_clean(cli.clean);

  tauri::Builder::default()
    .setup(move |app| {
      let app_handle = app.handle().clone();

      tauri::async_runtime::spawn(async move {
        let auth_result = authenticator.webview_authenticate(&app_handle).await;
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
