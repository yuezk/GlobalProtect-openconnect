pub mod xml;

pub mod base64;
pub mod checksum;
pub mod crypto;
pub mod endpoint;
pub mod env_utils;
mod identity_encoding;
mod legacy_pkcs8;
pub mod lock_file;
#[cfg(not(target_os = "macos"))]
pub mod openssl;
#[cfg(target_os = "macos")]
#[path = "pkcs12_identity/macos.rs"]
mod pkcs12_identity;
#[cfg(not(target_os = "macos"))]
#[path = "pkcs12_identity/openssl.rs"]
mod pkcs12_identity;
pub mod redact;
pub mod request;
#[cfg(target_os = "macos")]
#[path = "traditional_key/macos.rs"]
mod traditional_key;
#[cfg(not(target_os = "macos"))]
#[path = "traditional_key/openssl.rs"]
mod traditional_key;
#[cfg(feature = "tauri")]
pub mod window;

mod shutdown_signal;

use log::warn;
pub use shutdown_signal::shutdown_signal;

use reqwest::{Response, StatusCode, Url};
use thiserror::Error;

/// Normalize the server URL to the format `https://<host>:<port>`
pub fn normalize_server(server: &str) -> anyhow::Result<String> {
  let server = if server.starts_with("https://") || server.starts_with("http://") {
    server.to_string()
  } else {
    format!("https://{}", server)
  };

  let normalized_url = Url::parse(&server)?;
  let scheme = normalized_url.scheme();
  let host = normalized_url
    .host_str()
    .ok_or(anyhow::anyhow!("Invalid server URL: missing host"))?;

  let port: String = normalized_url.port().map_or("".into(), |port| format!(":{}", port));

  let normalized_url = format!("{}://{}{}", scheme, host, port);

  Ok(normalized_url)
}

pub fn remove_url_scheme(s: &str) -> String {
  s.replace("http://", "").replace("https://", "")
}

#[derive(Error, Debug)]
#[error("GP response error: reason={reason}, status={status}, body={body}")]
pub struct GpError {
  pub status: StatusCode,
  pub reason: String,
  body: String,
  #[source]
  source: Option<reqwest::Error>,
}

impl GpError {
  pub fn is_status_error(&self) -> bool {
    self.status.is_client_error() || self.status.is_server_error()
  }
}

pub async fn parse_gp_response(res: Response) -> anyhow::Result<String, GpError> {
  let status = res.status();

  if status.is_client_error() || status.is_server_error() {
    return Err(parse_gp_error(res).await);
  }

  res.text().await.map_err(|err| {
    warn!("Failed to read response: {}", err);

    GpError {
      status,
      reason: "failed to read response".to_string(),
      body: "<failed to read response>".to_string(),
      source: Some(err),
    }
  })
}

async fn parse_gp_error(res: Response) -> GpError {
  let status = res.status();
  let reason = res
    .headers()
    .get("x-private-pan-globalprotect")
    .map_or_else(|| "<none>", |v| v.to_str().unwrap_or("<invalid header>"))
    .to_string();

  let (body, source) = match res.text().await {
    Ok(body) => (if body.is_empty() { "<empty>".to_string() } else { body }, None),
    Err(error) => ("<failed to read response>".to_string(), Some(error)),
  };

  GpError {
    status,
    reason,
    body,
    source,
  }
}
