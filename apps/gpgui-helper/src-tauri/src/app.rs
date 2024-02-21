use std::sync::Arc;

use gpapi::utils::window::WindowExt;
use log::info;
use tauri::Manager;

use crate::updater::{GuiUpdater, Installer, ProgressNotifier};

pub struct App {
  api_key: Vec<u8>,
  gui_version: String,
}

impl App {
  pub fn new(api_key: Vec<u8>, gui_version: &str) -> Self {
    Self {
      api_key,
      gui_version: gui_version.to_string(),
    }
  }

  pub fn run(&self) -> anyhow::Result<()> {
    let gui_version = self.gui_version.clone();
    let api_key = self.api_key.clone();

    tauri::Builder::default()
      .setup(move |app| {
        let win = app.get_window("main").expect("no main window");
        win.hide_menu();

        let notifier = ProgressNotifier::new(win.clone());
        let installer = Installer::new(api_key);
        let updater = Arc::new(GuiUpdater::new(gui_version, notifier, installer));

        let win_clone = win.clone();
        app.listen_global("app://update-done", move |_event| {
          info!("Update done");
          let _ = win_clone.close();
        });

        // Listen for the update event
        win.listen("app://update", move |_event| {
          let updater = Arc::clone(&updater);
          tokio::spawn(async move { updater.update().await });
        });

        // Update the GUI on startup
        win.trigger("app://update", None);

        Ok(())
      })
      .run(tauri::generate_context!())?;

    Ok(())
  }
}
