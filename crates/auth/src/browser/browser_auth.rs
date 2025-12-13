use std::{env::temp_dir, fs};

use common::constants::GP_CALLBACK_PORT_FILENAME;
use gpapi::auth::SamlAuthData;
use log::info;
use tokio::{
  io::AsyncReadExt,
  net::{TcpListener, UdpSocket},
};

use crate::browser::auth_server::AuthServer;

pub enum Browser<'a> {
  Default,
  Chrome,
  Firefox,
  Remote,
  Other(&'a str),
}

impl<'a> Browser<'a> {
  pub fn from_str(browser: &'a str) -> Self {
    match browser.to_lowercase().as_str() {
      "default" => Browser::Default,
      "chrome" => Browser::Chrome,
      "firefox" => Browser::Firefox,
      "remote" => Browser::Remote,
      _ => Browser::Other(browser),
    }
  }

  fn as_str(&self) -> &str {
    match self {
      Browser::Default => "default",
      Browser::Chrome => "chrome",
      Browser::Firefox => "firefox",
      Browser::Remote => "remote",
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
    let addr = self.determine_addr().await?;
    let auth_server = AuthServer::new(&addr)?;
    let auth_url = auth_server.auth_url();

    let auth_request = self.auth_request.to_string();
    tokio::spawn(async move {
      auth_server.serve_request(&auth_request);
    });

    match self.browser {
      Browser::Remote => {
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

  async fn determine_addr(&self) -> anyhow::Result<String> {
    if matches!(self.browser, Browser::Remote) {
      let local_ip = detect_local_ip().await?;
      Ok(format!("{}:0", local_ip))
    } else {
      Ok("127.0.0.1:0".to_string())
    }
  }
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
  #[cfg(unix)]
  {
    use os::unix::fs::PermissionsExt;
    fs::set_permissions(&port_file, fs::Permissions::from_mode(0o600))?;
  }

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

fn read_auth_data_from_stdin() -> anyhow::Result<SamlAuthData> {
  let mut data = String::new();
  std::io::stdin().read_line(&mut data)?;

  let auth_data = SamlAuthData::from_gpcallback(data.trim())?;
  Ok(auth_data)
}
