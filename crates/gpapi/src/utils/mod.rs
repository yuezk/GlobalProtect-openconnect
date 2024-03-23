use reqwest::{Response, Url};

pub(crate) mod xml;

pub mod base64;
pub mod checksum;
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

  let port: String = normalized_url.port().map_or("".into(), |port| format!(":{}", port));

  let normalized_url = format!("{}://{}{}", scheme, host, port);

  Ok(normalized_url)
}

pub fn remove_url_scheme(s: &str) -> String {
  s.replace("http://", "").replace("https://", "")
}

pub(crate) async fn parse_gp_error(res: Response) -> (String, String) {
  let reason = res
    .headers()
    .get("x-private-pan-globalprotect")
    .map_or_else(|| "<none>", |v| v.to_str().unwrap_or("<invalid header>"))
    .to_string();

  let res = res.text().await.map_or_else(
    |_| "<failed to read response>".to_string(),
    |v| if v.is_empty() { "<empty>".to_string() } else { v },
  );

  (reason, res)
}
