use std::{io::Write, time::Duration};

use futures_util::StreamExt;
use log::info;
use tauri::{window::MenuHandle, Manager, Window};
use tempfile::NamedTempFile;

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
        hide_menu(win.menu_handle());

        tauri::async_runtime::spawn(async move {
          let win_clone = win.clone();
          download_gui(win.clone()).await;

          win.listen("app://download", move |_event| {
            tokio::spawn(download_gui(win_clone.clone()));
          });
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

async fn download_gui(win: Window) {
  let url = "https://github.com/yuezk/GlobalProtect-openconnect/releases/download/v2.0.0/globalprotect-openconnect_2.0.0_x86_64.bin.tar.gz";
  // let url = "https://free.nchc.org.tw/opensuse/distribution/leap/15.5/iso/openSUSE-Leap-15.5-DVD-x86_64-Build491.1-Media.iso";

  let win_clone = win.clone();

  match download(url, move |p| {
    let _ = win.emit_all("app://download-progress", p);
  })
  .await
  {
    Err(err) => {
      info!("download error: {}", err);
      let _ = win_clone.emit_all("app://download-error", ());
    }
    Ok(file) => {
      let path = file.into_temp_path();
      info!("download completed: {:?}", path);
      // Close window after 300ms
      tokio::time::sleep(Duration::from_millis(300 * 1000)).await;
      info!("file: {:?}", path);
      let _ = win_clone.close();
    }
  }
}

async fn download<T>(url: &str, on_progress: T) -> anyhow::Result<NamedTempFile>
where
  T: Fn(Option<f64>) + Send + 'static,
{
  let res = reqwest::get(url).await?.error_for_status()?;
  let content_length = res.content_length().unwrap_or(0);

  info!("content_length: {}", content_length);

  let mut current_length = 0;
  let mut stream = res.bytes_stream();

  let mut file = NamedTempFile::new()?;

  while let Some(item) = stream.next().await {
    let chunk = item?;
    let chunk_size = chunk.len() as u64;

    file.write_all(&chunk)?;

    current_length += chunk_size;
    let progress = current_length as f64 / content_length as f64 * 100.0;

    if content_length > 0 {
      on_progress(Some(progress));
    } else {
      on_progress(None);
    }
  }

  Ok(file)
}
