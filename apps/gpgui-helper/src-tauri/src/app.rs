use std::{sync::Arc, time::Duration};

use downloader::{progress::Reporter, Download, Downloader};
use tauri::{window::MenuHandle, Manager};
use tempfile::TempDir;

pub struct App {
  api_key: Vec<u8>,
}

impl App {
  pub fn new(api_key: Vec<u8>) -> Self {
    Self { api_key }
  }

  pub fn run(&self) -> anyhow::Result<()> {
    tauri::Builder::default()
      .setup(|app| {
        let win = app.get_window("main").unwrap();

        tauri::async_runtime::spawn(async move {
          hide_menu(win.menu_handle());
          let _ = download_gui();
        });

        Ok(())
      })
      .run(tauri::generate_context!())?;

    Ok(())
  }
}

// Fix the bug that the menu bar is visible on Ubuntu 18.04
fn hide_menu(menu_handle: MenuHandle) {
  tokio::spawn(async move {
    loop {
      let menu_visible = menu_handle.is_visible().unwrap_or(false);

      if !menu_visible {
        break;
      }

      if menu_visible {
        let _ = menu_handle.hide();
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    }
  });
}

struct DownloadProgress {}

impl Reporter for DownloadProgress {
  fn setup(&self, max_progress: Option<u64>, message: &str) {
    println!("{}: {}", message, max_progress.unwrap_or(0));
  }

  fn progress(&self, current: u64) {
    println!("progress: {}", current);
  }

  fn set_message(&self, message: &str) {
    println!("message: {}", message);
  }

  fn done(&self) {
    println!("done")
  }
}

fn download_gui() -> anyhow::Result<()> {
  let tmp_dir = TempDir::new()?;
  let tmp_dir = tmp_dir.into_path();

  let mut downloader = Downloader::builder().download_folder(&tmp_dir).build()?;

  let dl = Download::new("https://github.com/yuezk/GlobalProtect-openconnect/releases/download/v2.0.0/globalprotect-openconnect_2.0.0_x86_64.bin.tar.gz");
  let progress = Arc::new(DownloadProgress {});
  let dl = dl.progress(progress);

  let result = downloader.download(&[dl])?;

  for r in result {
    match r {
      Ok(s) => println!("Downloaded: {}", s),
      Err(e) => println!("Error: {}", e),
    }
  }

  Ok(())
}
