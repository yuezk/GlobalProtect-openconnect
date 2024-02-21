use std::io::Write;

use anyhow::bail;
use futures_util::StreamExt;
use log::info;
use tempfile::NamedTempFile;
use tokio::sync::RwLock;

type OnProgress = Box<dyn Fn(Option<f64>) + Send + Sync + 'static>;

pub struct FileDownloader<'a> {
  url: &'a str,
  on_progress: RwLock<Option<OnProgress>>,
}

impl<'a> FileDownloader<'a> {
  pub fn new(url: &'a str) -> Self {
    Self {
      url,
      on_progress: Default::default(),
    }
  }

  pub fn on_progress<T>(&self, on_progress: T)
  where
    T: Fn(Option<f64>) + Send + Sync + 'static,
  {
    if let Ok(mut guard) = self.on_progress.try_write() {
      *guard = Some(Box::new(on_progress));
    } else {
      info!("Failed to acquire on_progress lock");
    }
  }

  pub async fn download(&self) -> anyhow::Result<NamedTempFile> {
    let res = reqwest::get(self.url).await?.error_for_status()?;
    let content_length = res.content_length().unwrap_or(0);

    info!("Content length: {}", content_length);

    let mut current_length = 0;
    let mut stream = res.bytes_stream();

    let mut file = NamedTempFile::new()?;

    while let Some(item) = stream.next().await {
      let chunk = item?;
      let chunk_size = chunk.len() as u64;

      file.write_all(&chunk)?;

      current_length += chunk_size;
      let progress = current_length as f64 / content_length as f64 * 100.0;

      if let Some(on_progress) = &*self.on_progress.read().await {
        let progress = if content_length > 0 { Some(progress) } else { None };

        on_progress(progress);
      }
    }

    if content_length > 0 && current_length != content_length {
      bail!("Download incomplete");
    }

    info!("Downloaded to: {:?}", file.path());

    Ok(file)
  }
}

pub struct ChecksumFetcher<'a> {
  url: &'a str,
}

impl<'a> ChecksumFetcher<'a> {
  pub fn new(url: &'a str) -> Self {
    Self { url }
  }

  pub async fn fetch(&self) -> anyhow::Result<String> {
    let res = reqwest::get(self.url).await?.error_for_status()?;
    let checksum = res.text().await?.trim().to_string();

    Ok(checksum)
  }
}
