use reqwest::Url;

pub(crate) mod xml;

pub mod base64;
pub mod crypto;
pub mod endpoint;
pub mod env_file;
pub mod lock_file;
pub mod openssl;
pub mod redact;
#[cfg(feature = "tauri")]
pub mod window;

mod shutdown_signal;

pub use shutdown_signal::shutdown_signal;

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

  let port: String = normalized_url
    .port()
    .map_or("".into(), |port| format!(":{}", port));

  let normalized_url = format!("{}://{}{}", scheme, host, port);

  Ok(normalized_url)
}
