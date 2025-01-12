use super::lock_file::gpservice_lock_info;

async fn read_port() -> anyhow::Result<String> {
  let lock_info = gpservice_lock_info().await?;

  Ok(lock_info.port.to_string())
}

pub async fn http_endpoint() -> anyhow::Result<String> {
  let port = read_port().await?;

  Ok(format!("http://127.0.0.1:{}", port))
}

pub async fn ws_endpoint() -> anyhow::Result<String> {
  let port = read_port().await?;

  Ok(format!("ws://127.0.0.1:{}/ws", port))
}
