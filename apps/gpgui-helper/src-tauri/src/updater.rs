use std::sync::Arc;

use gpapi::{
  service::request::UpdateGuiRequest,
  utils::{checksum::verify_checksum, crypto::Crypto, endpoint::http_endpoint},
};
use log::{info, warn};
use tauri::{Manager, Window};

use crate::downloader::{ChecksumFetcher, FileDownloader};

pub struct ProgressNotifier {
  win: Window,
}

impl ProgressNotifier {
  pub fn new(win: Window) -> Self {
    Self { win }
  }

  fn notify(&self, progress: Option<f64>) {
    let _ = self.win.emit_all("app://update-progress", progress);
  }

  fn notify_error(&self) {
    let _ = self.win.emit_all("app://update-error", ());
  }

  fn notify_done(&self) {
    let _ = self.win.emit_and_trigger("app://update-done", ());
  }
}

pub struct Installer {
  crypto: Crypto,
}

impl Installer {
  pub fn new(api_key: Vec<u8>) -> Self {
    Self {
      crypto: Crypto::new(api_key),
    }
  }

  async fn install(&self, path: &str, checksum: &str) -> anyhow::Result<()> {
    let service_endpoint = http_endpoint().await?;

    let request = UpdateGuiRequest {
      path: path.to_string(),
      checksum: checksum.to_string(),
    };
    let payload = self.crypto.encrypt(&request)?;

    reqwest::Client::default()
      .post(format!("{}/update-gui", service_endpoint))
      .body(payload)
      .send()
      .await?
      .error_for_status()?;

    Ok(())
  }
}

pub struct GuiUpdater {
  version: String,
  notifier: Arc<ProgressNotifier>,
  installer: Installer,
}

impl GuiUpdater {
  pub fn new(version: String, notifier: ProgressNotifier, installer: Installer) -> Self {
    Self {
      version,
      notifier: Arc::new(notifier),
      installer,
    }
  }

  pub async fn update(&self) {
    info!("Update GUI, version: {}", self.version);

    #[cfg(debug_assertions)]
    let release_tag = "latest";
    #[cfg(not(debug_assertions))]
    let release_tag = format!("v{}", self.version);

    #[cfg(target_arch = "x86_64")]
    let arch = "x86_64";
    #[cfg(target_arch = "aarch64")]
    let arch = "aarch64";

    let file_url = format!(
      "https://github.com/yuezk/GlobalProtect-openconnect/releases/download/{}/gpgui_{}_{}.bin.tar.xz",
      release_tag, self.version, arch
    );
    let checksum_url = format!("{}.sha256", file_url);

    info!("Downloading file: {}", file_url);

    let dl = FileDownloader::new(&file_url);
    let cf = ChecksumFetcher::new(&checksum_url);
    let notifier = Arc::clone(&self.notifier);

    dl.on_progress(move |progress| notifier.notify(progress));

    let res = tokio::try_join!(dl.download(), cf.fetch());

    let (file, checksum) = match res {
      Ok((file, checksum)) => (file, checksum),
      Err(err) => {
        warn!("Download error: {}", err);
        self.notifier.notify_error();
        return;
      }
    };

    let path = file.into_temp_path();
    let file_path = path.to_string_lossy();

    if let Err(err) = verify_checksum(&file_path, &checksum) {
      warn!("Checksum error: {}", err);
      self.notifier.notify_error();
      return;
    }

    info!("Checksum success");

    if let Err(err) = self.installer.install(&file_path, &checksum).await {
      warn!("Install error: {}", err);
      self.notifier.notify_error();
    } else {
      info!("Install success");
      self.notifier.notify_done();
    }
  }
}
