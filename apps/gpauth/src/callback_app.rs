use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

use tauri::RunEvent;

pub fn run() -> anyhow::Result<()> {
  let handled = Arc::new(AtomicBool::new(false));
  let timeout_handled = Arc::clone(&handled);
  let app = tauri::Builder::default()
    .setup(move |app| {
      let app_handle = app.handle().clone();
      tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        if !timeout_handled.swap(true, Ordering::AcqRel) {
          app_handle.exit(1);
        }
      });
      Ok(())
    })
    .build(crate::tauri_context::get())?;

  app.run(move |app_handle, event| {
    let RunEvent::Opened { urls } = event else {
      return;
    };
    let Some(callback) = urls
      .into_iter()
      .find(|url| url.scheme().eq_ignore_ascii_case("globalprotectcallback"))
    else {
      return;
    };
    if handled.swap(true, Ordering::AcqRel) {
      return;
    }

    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
      let exit_code = match auth::forward_auth_callback(callback.as_str()).await {
        Ok(()) => 0,
        Err(err) => {
          eprintln!("Failed to forward authentication callback: {err}");
          1
        }
      };
      app_handle.exit(exit_code);
    });
  });

  Ok(())
}
