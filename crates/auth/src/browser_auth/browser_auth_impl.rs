use std::{env::temp_dir, fs, os::unix::fs::PermissionsExt};

use gpapi::{auth::SamlAuthData, GP_CALLBACK_PORT_FILENAME};
use log::info;
use tokio::{io::AsyncReadExt, net::TcpListener};

use super::auth_server::AuthServer;

pub(super) struct BrowserAuthenticatorImpl<'a> {
  auth_request: &'a str,
  browser: Option<&'a str>,
}

impl BrowserAuthenticatorImpl<'_> {
  pub fn new(auth_request: &str) -> BrowserAuthenticatorImpl {
    BrowserAuthenticatorImpl {
      auth_request,
      browser: None,
    }
  }

  pub fn new_with_browser<'a>(auth_request: &'a str, browser: &'a str) -> BrowserAuthenticatorImpl<'a> {
    let browser = browser.trim();
    BrowserAuthenticatorImpl {
      auth_request,
      browser: if browser.is_empty() || browser == "default" {
        None
      } else {
        Some(browser)
      },
    }
  }

  pub async fn authenticate(&self) -> anyhow::Result<SamlAuthData> {
    let auth_server = AuthServer::new()?;
    let auth_url = auth_server.auth_url();

    let auth_request = self.auth_request.to_string();
    tokio::spawn(async move {
      auth_server.serve_request(&auth_request);
    });

    if let Some(browser) = self.browser {
      let app = find_browser_path(browser);

      info!("Launching browser: {}", app);
      open::with_detached(auth_url, app)?;
    } else {
      info!("Launching the default browser...");
      webbrowser::open(&auth_url)?;
    }

    info!("Please continue the authentication process in the default browser");
    wait_auth_data().await
  }
}

fn find_browser_path(browser: &str) -> String {
  if browser == "chrome" {
    which::which("google-chrome-stable")
      .or_else(|_| which::which("google-chrome"))
      .or_else(|_| which::which("chromium"))
      .map(|path| path.to_string_lossy().to_string())
      .unwrap_or_else(|_| browser.to_string())
  } else {
    browser.into()
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
