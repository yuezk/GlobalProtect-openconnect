use auth::WebviewAuthenticatorBuilder;
use log::info;
use tao::{
  event::{Event, WindowEvent},
  event_loop::{ControlFlow, EventLoopBuilder},
};
use tempfile::NamedTempFile;

use crate::cli::{print_auth_result, Cli};

pub async fn authenticate<'a>(
  builder: WebviewAuthenticatorBuilder<'a>,
  mut openssl_conf: Option<NamedTempFile>,
) -> anyhow::Result<()> {
  let event_loop = EventLoopBuilder::with_user_event().build();
  let authenticator = builder.build(&event_loop)?;

  authenticator.authenticate().await?;

  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;

    if let Event::WindowEvent {
      event: WindowEvent::CloseRequested,
      ..
    } = event
    {
      *control_flow = ControlFlow::Exit;
      if let Some(file) = openssl_conf.take() {
        if let Err(err) = file.close() {
          info!("Error closing OpenSSL config file: {}", err);
        }
      }
    }
  });

  Ok(())

  // tauri::Builder::default()
  //   .setup(move |app| {
  //     let app_handle = app.handle().clone();

  //     tauri::async_runtime::spawn(async move {
  //       let auth_result = authenticator.webview_authenticate(&app_handle).await;
  //       print_auth_result(auth_result);

  //       // Ensure the app exits after the authentication process
  //       app_handle.exit(0);
  //     });

  //     Ok(())
  //   })
  //   .build(tauri::generate_context!())?
  //   .run(move |_app_handle, event| {
  //     if let RunEvent::Exit = event {
  //       if let Some(file) = openssl_conf.take() {
  //         if let Err(err) = file.close() {
  //           info!("Error closing OpenSSL config file: {}", err);
  //         }
  //       }
  //     }
  //   });
}
