use tauri::WebviewWindow;

pub trait WindowExt {
  fn raise(&self) -> anyhow::Result<()>;
}

impl WindowExt for WebviewWindow {
  #[cfg(any(target_os = "macos", target_os = "windows"))]
  fn raise(&self) -> anyhow::Result<()> {
    self.show()?;
    Ok(())
  }

  #[cfg(all(not(any(target_os = "macos", target_os = "windows")), feature = "webview-auth"))]
  fn raise(&self) -> anyhow::Result<()> {
    unix::raise_window(self)
  }

  #[cfg(all(not(any(target_os = "macos", target_os = "windows")), not(feature = "webview-auth")))]
  fn raise(&self) -> anyhow::Result<()> {
    // Fallback implementation without GTK
    self.show()?;
    Ok(())
  }
}

#[cfg(all(not(any(target_os = "macos", target_os = "windows")), feature = "webview-auth"))]
mod unix {
  use std::{process::ExitStatus, time::Duration};

  use anyhow::bail;
  use gtk::{
    glib::Cast,
    traits::{EventBoxExt, GtkWindowExt, WidgetExt},
    EventBox,
  };
  use log::info;
  use tauri::WebviewWindow;
  use tokio::process::Command;

  pub fn raise_window(win: &WebviewWindow) -> anyhow::Result<()> {
    let is_wayland = std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";

    if is_wayland {
      let gtk_win = win.gtk_window()?;
      if let Some(header) = gtk_win.titlebar() {
        let _ = header.downcast::<EventBox>().map(|event_box| {
          event_box.set_above_child(false);
        });
      }

      gtk_win.hide();
      gtk_win.show_all();
    } else {
      if !win.is_visible()? {
        win.show()?;
      }
      let title = win.title()?;
      tokio::spawn(async move {
        if let Err(err) = wmctrl_raise_window(&title).await {
          info!("Window not raised: {}", err);
        }
      });
    }

    // Calling window.show() on window object will cause the menu to be shown.
    // We need to hide it again.
    win.hide_menu()?;

    Ok(())
  }

  async fn wmctrl_raise_window(title: &str) -> anyhow::Result<()> {
    let mut counter = 0;

    loop {
      if let Ok(exit_status) = wmctrl_try_raise_window(title).await {
        if exit_status.success() {
          info!("Window raised after {} attempts", counter + 1);
          return Ok(());
        }
      }

      if counter >= 10 {
        bail!("Failed to raise window: {}", title)
      }

      counter += 1;
      tokio::time::sleep(Duration::from_millis(100)).await;
    }
  }

  async fn wmctrl_try_raise_window(title: &str) -> anyhow::Result<ExitStatus> {
    let exit_status = Command::new("wmctrl")
      .arg("-F")
      .arg("-a")
      .arg(title)
      .spawn()?
      .wait()
      .await?;

    Ok(exit_status)
  }
}
