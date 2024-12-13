use std::{process::ExitStatus, time::Duration};

use anyhow::bail;
use log::info;
use tauri::WebviewWindow;
use tokio::process::Command;

pub trait WindowExt {
  fn raise(&self) -> anyhow::Result<()>;
}

impl WindowExt for WebviewWindow {
  fn raise(&self) -> anyhow::Result<()> {
    raise_window(self)
  }
}

pub fn raise_window(win: &WebviewWindow) -> anyhow::Result<()> {
  let is_wayland = std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";

  if is_wayland {
    win.hide()?;
    win.show()?;
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

  // Calling window.show() on Windows will cause the menu to be shown.
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
