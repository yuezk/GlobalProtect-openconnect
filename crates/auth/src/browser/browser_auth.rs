use std::{env::temp_dir, fs, os::unix::fs::PermissionsExt};

use gpapi::{auth::SamlAuthData, GP_CALLBACK_PORT_FILENAME};
use log::info;
use tokio::{io::AsyncReadExt, net::TcpListener};

use crate::browser::auth_server::AuthServer;

pub enum Browser<'a> {
  Default,
  Chrome,
  Firefox,
  Other(&'a str),
}

impl<'a> Browser<'a> {
  pub fn from_str(browser: &'a str) -> Self {
    match browser.to_lowercase().as_str() {
      "default" => Browser::Default,
      "chrome" => Browser::Chrome,
      "firefox" => Browser::Firefox,
      _ => Browser::Other(browser),
    }
  }

  fn as_str(&self) -> &str {
    match self {
      Browser::Default => "default",
      Browser::Chrome => "chrome",
      Browser::Firefox => "firefox",
      Browser::Other(browser) => browser,
    }
  }
}

pub struct BrowserAuthenticator<'a> {
  auth_request: &'a str,
  browser: Browser<'a>,
}

impl<'a> BrowserAuthenticator<'a> {
  pub fn new(auth_request: &'a str, browser: &'a str) -> Self {
    Self {
      auth_request,
      browser: Browser::from_str(browser),
    }
  }

  pub async fn authenticate(&self) -> anyhow::Result<SamlAuthData> {
    let auth_server = AuthServer::new()?;
    let auth_url = auth_server.auth_url();

    let auth_request = self.auth_request.to_string();
    tokio::spawn(async move {
      auth_server.serve_request(&auth_request);
    });

    match self.browser {
      Browser::Default => {
        info!("Launching the default browser...");
        webbrowser::open(&auth_url)?;
      }
      _ => {
        let app = find_browser_path(&self.browser);

        info!("Launching browser: {}", app);
        open::with_detached(auth_url, app)?;
      }
    }

    info!("Please continue the authentication process in the default browser");
    wait_auth_data().await
  }
}

fn find_browser_path(browser: &Browser) -> String {
  match browser {
    Browser::Chrome => {
      const CHROME_VARIANTS: &[&str] = &["google-chrome-stable", "google-chrome", "chromium"];

      CHROME_VARIANTS
        .iter()
        .find_map(|&browser_name| which::which(browser_name).ok())
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| browser.as_str().to_string())
    }
    _ => browser.as_str().to_string(),
  }
}

async fn wait_auth_data() -> anyhow::Result<SamlAuthData> {
  // Start a local server to receive the browser authentication data
  let listener = TcpListener::bind("127.0.0.1:0").await?;
  let port = listener.local_addr()?.port();
  let port_file = temp_dir().join(GP_CALLBACK_PORT_FILENAME);

  // Write the port to a file
  fs::write(&port_file, port.to_string())?;
  fs::set_permissions(&port_file, fs::Permissions::from_mode(0o600))?;

  // Remove the previous log file
  let callback_log = temp_dir().join("gpcallback.log");
  let _ = fs::remove_file(&callback_log);

  info!("Listening authentication data on port {}", port);
  info!(
    "If it hangs, please check the logs at `{}` for more information",
    callback_log.display()
  );
  let (mut socket, _) = listener.accept().await?;

  info!("Received the browser authentication data from the socket");
  let mut data = String::new();
  socket.read_to_string(&mut data).await?;

  // Remove the port file
  fs::remove_file(&port_file)?;

  let auth_data = SamlAuthData::from_gpcallback(&data)?;
  Ok(auth_data)
}
