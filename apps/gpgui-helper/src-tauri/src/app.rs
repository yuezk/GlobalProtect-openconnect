use std::sync::Arc;

use gpapi::service::transport::SessionCredential;
use log::info;
use tauri::{Listener, Manager};
use tokio::sync::watch;

use crate::updater::{GuiUpdater, Installer, ProgressNotifier};

pub struct App {
  credential: Arc<SessionCredential>,
  credential_lease: watch::Receiver<bool>,
  gui_version: String,
}

impl App {
  pub fn new(credential: Arc<SessionCredential>, credential_lease: watch::Receiver<bool>, gui_version: &str) -> Self {
    Self {
      credential,
      credential_lease,
      gui_version: gui_version.to_string(),
    }
  }

  pub fn run(&self) -> anyhow::Result<()> {
    let gui_version = self.gui_version.clone();
    let credential = Arc::clone(&self.credential);
    let credential_lease = self.credential_lease.clone();

    tauri::Builder::default()
      .setup(move |app| {
        let win = app.get_webview_window("main").expect("no main window");
        let _ = win.hide_menu();

        let notifier = ProgressNotifier::new(win.clone());
        let installer = Installer::new(credential, credential_lease.clone());
        let updater = Arc::new(GuiUpdater::new(gui_version, notifier, installer));

        let app_handle = app.handle().clone();
        let mut app_credential_lease = credential_lease;
        tokio::spawn(async move {
          while *app_credential_lease.borrow() {
            if app_credential_lease.changed().await.is_err() {
              return;
            }
          }
          info!("Service credential lease ended");
          app_handle.exit(0);
        });

        let win_clone = win.clone();
        app.listen_any("app://update-done", move |_event| {
          info!("Update done");
          let _ = win_clone.close();
        });

        // Listen for the update event
        win.listen("app://update", move |_event| {
          let updater = Arc::clone(&updater);
          if updater.is_in_progress() {
            info!("Update already in progress");
            updater.notify_progress();
            return;
          }

          tokio::spawn(async move { updater.update().await });
        });

        Ok(())
      })
      .run(tauri::generate_context!())?;

    Ok(())
  }
}
