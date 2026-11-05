use std::sync::Arc;

use gpapi::{gui::GuiTheme, service::transport::SessionCredential};
use log::info;
use tauri::{Listener, Manager};
use tokio::sync::watch;

use crate::updater::{GuiUpdater, Installer, ProgressNotifier};

pub struct App {
  credential: Arc<SessionCredential>,
  credential_lease: watch::Receiver<bool>,
  gui_version: String,
  variant: gpapi::gui::GuiVariant,
  theme: GuiTheme,
}

impl App {
  pub fn new(
    credential: Arc<SessionCredential>,
    credential_lease: watch::Receiver<bool>,
    gui_version: &str,
    variant: gpapi::gui::GuiVariant,
    theme: GuiTheme,
  ) -> Self {
    Self {
      credential,
      credential_lease,
      gui_version: gui_version.to_string(),
      variant,
      theme,
    }
  }

  pub fn run(&self) -> anyhow::Result<()> {
    let gui_version = self.gui_version.clone();
    let variant = self.variant;
    let theme = self.theme;
    let credential = Arc::clone(&self.credential);
    let credential_lease = self.credential_lease.clone();

    tauri::Builder::default()
      .on_window_event(move |window, event| {
        if theme == GuiTheme::System
          && let tauri::WindowEvent::ThemeChanged(theme) = event
          && let Some(win) = window.get_webview_window(window.label())
          && let Err(error) = win.set_background_color(Some(background_color(*theme)))
        {
          log::warn!("Could not update helper window background: {error}");
        }
      })
      .setup(move |app| {
        let native_theme = match theme {
          GuiTheme::System => None,
          GuiTheme::Light => Some(tauri::Theme::Light),
          GuiTheme::Dark => Some(tauri::Theme::Dark),
        };
        let config = app
          .config()
          .app
          .windows
          .iter()
          .find(|window| window.label == "main")
          .expect("no main window");
        let win = tauri::WebviewWindowBuilder::from_config(app, config)?
          .theme(native_theme)
          .initialization_script(format!("window.__GP_HELPER_THEME__ = {:?};", theme.as_str()))
          .visible(false)
          .build()?;
        win.set_background_color(Some(background_color(win.theme()?)))?;
        let _ = win.hide_menu();

        let notifier = ProgressNotifier::new(win.clone());
        let installer = Installer::new(credential, credential_lease.clone());
        let updater = Arc::new(GuiUpdater::new(gui_version, notifier, installer, variant));

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

        win.show()?;

        Ok(())
      })
      .run(tauri::generate_context!())?;

    Ok(())
  }
}

fn background_color(theme: tauri::Theme) -> tauri::window::Color {
  match theme {
    tauri::Theme::Dark => tauri::window::Color(36, 36, 36, 255),
    _ => tauri::window::Color(246, 245, 244, 255),
  }
}
