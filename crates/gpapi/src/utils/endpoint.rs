use std::path::Path;

use super::lock_file::{gpservice_lock_info, gpservice_lock_info_at};

pub async fn http_endpoint() -> anyhow::Result<String> {
  let port = gpservice_lock_info().await?.port;

  Ok(format!("http://127.0.0.1:{}", port))
}

pub async fn ws_endpoint() -> anyhow::Result<String> {
  let port = gpservice_lock_info().await?.port;

  Ok(format!("ws://127.0.0.1:{}/ws", port))
}

pub async fn ws_endpoint_from_lock_file(path: impl AsRef<Path>) -> anyhow::Result<String> {
  let port = gpservice_lock_info_at(path).await?.port;

  Ok(format!("ws://127.0.0.1:{}/ws", port))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn websocket_endpoint_uses_the_selected_lock_file() {
    let temp = tempfile::tempdir().unwrap();
    let lock_file = temp.path().join("gpservice.lock");
    std::fs::write(&lock_file, "123:4242").unwrap();

    assert_eq!(
      ws_endpoint_from_lock_file(lock_file).await.unwrap(),
      "ws://127.0.0.1:4242/ws"
    );
  }
}
