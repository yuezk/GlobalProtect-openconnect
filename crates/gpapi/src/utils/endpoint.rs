use tokio::fs;

use crate::GP_SERVICE_LOCK_FILE;

async fn read_port() -> anyhow::Result<String> {
  let port = fs::read_to_string(GP_SERVICE_LOCK_FILE).await?;
  Ok(port.trim().to_string())
}

pub async fn http_endpoint() -> anyhow::Result<String> {
  let port = read_port().await?;

  Ok(format!("http://127.0.0.1:{}", port))
}

pub async fn ws_endpoint() -> anyhow::Result<String> {
  let port = read_port().await?;

  Ok(format!("ws://127.0.0.1:{}/ws", port))
}
