pub mod xml;

pub mod base64;
mod certificate;
pub mod checksum;
mod client_tls;
pub mod crypto;
#[cfg(feature = "desktop")]
pub mod endpoint;
#[cfg(feature = "desktop")]
pub mod env_utils;
mod identity_encoding;
#[cfg(feature = "desktop")]
pub mod lock_file;
pub mod redact;
mod response_log;
pub(crate) use response_log::log_response_xml;
pub mod request;
#[cfg(feature = "tauri")]
pub mod window;

#[cfg(feature = "desktop")]
mod shutdown_signal;

use log::warn;
#[cfg(feature = "desktop")]
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
#[error("GP response error: reason={reason}, status={status}")]
pub struct GpError {
  pub status: StatusCode,
  pub reason: String,
  #[source]
  source: Option<reqwest::Error>,
}

impl GpError {
  pub(crate) fn take_body_read_error(&mut self) -> Option<reqwest::Error> {
    if self.is_status_error() {
      None
    } else {
      self.source.take()
    }
  }

  pub fn is_status_error(&self) -> bool {
    self.status.is_client_error() || self.status.is_server_error()
  }

  pub fn is_authentication_rejection(&self) -> bool {
    matches!(self.status.as_u16(), 401 | 403)
  }
}

pub async fn parse_gp_response(res: Response) -> anyhow::Result<String, GpError> {
  let status = res.status();

  if status.is_client_error() || status.is_server_error() {
    return Err(parse_gp_error(res).await);
  }

  let path = log::log_enabled!(log::Level::Debug).then(|| res.url().path().to_owned());
  let body = res.text().await.map_err(|err| {
    warn!("Failed to read response: {}", err);

    GpError {
      status,
      reason: "failed to read response".to_string(),
      source: Some(err),
    }
  })?;
  if let Some(path) = path {
    log_response_xml(&path, status, body.as_bytes());
  }
  Ok(body)
}

async fn parse_gp_error(res: Response) -> GpError {
  let status = res.status();
  let reason = res
    .headers()
    .get("x-private-pan-globalprotect")
    .map_or_else(|| "<none>", |v| v.to_str().unwrap_or("<invalid header>"))
    .to_string();

  let path = log::log_enabled!(log::Level::Debug).then(|| res.url().path().to_owned());
  let source = match res.text().await {
    Ok(body) => {
      if let Some(path) = path {
        log_response_xml(&path, status, body.as_bytes());
      }
      None
    }
    Err(error) => Some(error),
  };

  GpError { status, reason, source }
}
