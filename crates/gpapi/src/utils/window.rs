use tauri::WebviewWindow;

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowTypeHint {
  Dialog,
  Utility,
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub fn set_window_type_hint(window: &WebviewWindow, hint: WindowTypeHint) -> anyhow::Result<()> {
  use gtk::{gdk, traits::GtkWindowExt};

  let hint = match hint {
    WindowTypeHint::Dialog => gdk::WindowTypeHint::Dialog,
    WindowTypeHint::Utility => gdk::WindowTypeHint::Utility,
  };

  window.gtk_window()?.set_type_hint(hint);
  Ok(())
}

/// Returns the frame-to-content height difference for a standard titled macOS window.
/// This is nonzero only on macOS.
pub fn get_title_bar_height() -> f64 {
  #[cfg(target_os = "macos")]
  {
    macos_title_bar_height(os_info::get().version())
  }
  #[cfg(not(target_os = "macos"))]
  {
    0.0
  }
}

#[cfg(target_os = "macos")]
fn macos_title_bar_height(version: &os_info::Version) -> f64 {
  if version.ge(&os_info::Version::from_string("26.0.0")) {
    32.0
  } else {
    28.0
  }
}

pub trait WindowExt {
  fn raise(&self) -> anyhow::Result<()>;
}

impl WindowExt for WebviewWindow {
  #[cfg(target_os = "macos")]
  fn raise(&self) -> anyhow::Result<()> {
    self.show()?;
    self.set_focus()?;
    Ok(())
  }

  #[cfg(target_os = "windows")]
  fn raise(&self) -> anyhow::Result<()> {
    self.show()?;
    Ok(())
  }

  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  fn raise(&self) -> anyhow::Result<()> {
    unix::raise_window(self)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(target_os = "macos")]
  #[test]
  fn uses_the_current_macos_title_bar_height() {
    assert_eq!(macos_title_bar_height(&os_info::Version::from_string("25.9.0")), 28.0);
    assert_eq!(macos_title_bar_height(&os_info::Version::from_string("26.0.0")), 32.0);
  }

  #[cfg(not(target_os = "macos"))]
  #[test]
  fn native_title_bar_height_is_zero_without_a_native_macos_title_bar() {
    assert_eq!(get_title_bar_height(), 0.0);
  }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod unix {
  use std::{process::ExitStatus, time::Duration};

  use anyhow::bail;
  use gtk::{
    EventBox,
    glib::{self, Cast},
    traits::{EventBoxExt, GtkWindowExt, WidgetExt},
  };
  use log::info;
  use tauri::WebviewWindow;
  use tokio::process::Command;

  /// Wrapper to mark a non-Send closure as Send for dispatch to the GTK main thread.
  ///
  /// SAFETY: The caller must ensure the closure is only executed on the GTK main
  /// thread. This is guaranteed by `glib::MainContext::default().invoke()`.
  struct SendFn(Box<dyn FnOnce()>);
  unsafe impl Send for SendFn {}

  impl SendFn {
    fn invoke(self) {
      (self.0)();
    }
  }

  pub fn raise_window(win: &WebviewWindow) -> anyhow::Result<()> {
    let is_wayland = std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";

    if is_wayland {
      let gtk_win = win.gtk_window()?;

      // GTK operations must run on the main thread. When this function is called
      // from a background thread (e.g., rspc handler on a tokio worker), calling
      // GTK widget methods directly causes crashes in GTK's CSS subsystem
      // (gtk_css_value_inherit_free assertion failure). Dispatch to the main
      // thread via glib::MainContext to ensure thread safety.
      let send_fn = SendFn(Box::new(move || {
        if let Some(header) = gtk_win.titlebar() {
          let _ = header.downcast::<EventBox>().map(|event_box| {
            event_box.set_above_child(false);
          });
        }

        gtk_win.hide();
        gtk_win.show_all();
      }));

      glib::MainContext::default().invoke(move || send_fn.invoke());
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
