use std::path::PathBuf;

pub struct LockFile {
  path: PathBuf,
}

impl LockFile {
  pub fn new<P: Into<PathBuf>>(path: P) -> Self {
    Self { path: path.into() }
  }

  pub fn exists(&self) -> bool {
    self.path.exists()
  }

  pub fn lock(&self, content: impl AsRef<[u8]>) -> anyhow::Result<()> {
    std::fs::write(&self.path, content)?;
    Ok(())
  }

  pub fn unlock(&self) -> anyhow::Result<()> {
    std::fs::remove_file(&self.path)?;
    Ok(())
  }

  pub async fn check_health(&self) -> bool {
    match std::fs::read_to_string(&self.path) {
      Ok(content) => {
        let url = format!("http://127.0.0.1:{}/health", content.trim());

        match reqwest::get(&url).await {
          Ok(resp) => resp.status().is_success(),
          Err(_) => false,
        }
      }
      Err(_) => false,
    }
  }
}
