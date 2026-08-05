use browser_launcher::Browser as LaunchBrowser;
use gpapi::auth::SamlAuthData;
use log::info;
use tokio::net::UdpSocket;

use crate::{AuthCallbackReceiver, browser::auth_server::AuthServer, ensure_auth_callback_handler};

pub struct BrowserAuthenticator<'a> {
  auth_request: &'a str,
  browser: &'a str,
}

impl<'a> BrowserAuthenticator<'a> {
  pub fn new(auth_request: &'a str, browser: &'a str) -> Self {
    Self { auth_request, browser }
  }

  pub async fn authenticate(&self) -> anyhow::Result<SamlAuthData> {
    let callback_receiver = if is_remote_browser(self.browser) {
      None
    } else {
      ensure_auth_callback_handler()?;
      Some(AuthCallbackReceiver::bind()?)
    };
    let addr = self.determine_addr().await?;
    let auth_server = AuthServer::new(&addr)?;
    let auth_url = auth_server.auth_url();

    let auth_request = self.auth_request.to_string();
    tokio::spawn(async move {
      auth_server.serve_request(&auth_request);
    });

    if is_remote_browser(self.browser) {
      info!(
        r#"

==== Manual Authentication Required ====

Please open the following URL in your browser:

{}

After completing the authentication, please paste the authentication data back to this terminal.
(The data should start with "globalprotectcallback:...")

Note that the URL is only valid for a single use.
"#,
        auth_url
      );
      return read_auth_data_from_stdin();
    }

    browser_launcher::open_url(&auth_url, LaunchBrowser::from_name(self.browser))?;

    info!("Please continue the authentication process in the default browser");
    callback_receiver.unwrap().receive().await
  }

  async fn determine_addr(&self) -> anyhow::Result<String> {
    if is_remote_browser(self.browser) {
      let local_ip = detect_local_ip().await?;
      Ok(format!("{}:0", local_ip))
    } else {
      Ok("127.0.0.1:0".to_string())
    }
  }
}

fn is_remote_browser(browser: &str) -> bool {
  browser.eq_ignore_ascii_case("remote")
}

/// Detect the local IP address by creating a UDP socket and connecting to an external address
async fn detect_local_ip() -> anyhow::Result<String> {
  let socket = UdpSocket::bind("0.0.0.0:0").await?;
  if let Err(err) = socket.connect("1.1.1.1:80").await {
    anyhow::bail!("Failed to connect to external address to determine local IP: {}", err);
  }
  let local_addr = socket.local_addr()?;
  let ip = local_addr.ip().to_string();
  info!("Determined local IP address: {}", ip);

  Ok(ip.to_string())
}

fn read_auth_data_from_stdin() -> anyhow::Result<SamlAuthData> {
  let mut data = String::new();
  std::io::stdin().read_line(&mut data)?;

  let auth_data = SamlAuthData::from_gpcallback(data.trim())?;
  Ok(auth_data)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shared_launcher_keeps_auto_distinct_from_system_default() {
    assert_eq!(LaunchBrowser::from_name("auto"), LaunchBrowser::Auto);
    assert_eq!(LaunchBrowser::from_name("default"), LaunchBrowser::Default);
  }

  #[test]
  fn browser_remote_remains_auth_specific() {
    assert!(is_remote_browser("remote"));
    assert!(is_remote_browser("REMOTE"));
    assert_ne!(LaunchBrowser::from_name("remote"), LaunchBrowser::Default);
  }
}
