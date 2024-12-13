use std::{
  borrow::Cow,
  env::temp_dir,
  fs,
  os::unix::fs::PermissionsExt,
  sync::Arc,
  time::{Duration, Instant},
};

use anyhow::bail;
use gpapi::{
  auth::SamlAuthData,
  error::PortalError,
  gp_params::GpParams,
  portal::{prelogin, Prelogin},
  process::browser_authenticator::BrowserAuthenticator,
  utils::window::WindowExt,
  GP_CALLBACK_PORT_FILENAME,
};
use log::{info, warn};
use tauri::{AppHandle, WebviewUrl, WebviewWindow, WindowEvent};
use tokio::{
  io::AsyncReadExt,
  net::TcpListener,
  sync::{oneshot, RwLock},
  time,
};

use crate::{
  auth_messenger::{AuthError, AuthEvent, AuthMessenger},
  common::{AuthRequest, AuthSettings},
  platform_impl,
};

pub struct AuthWindow<'a> {
  server: &'a str,
  gp_params: &'a GpParams,
  auth_request: Option<&'a str>,
  clean: bool,
  is_retrying: RwLock<bool>,
}

impl<'a> AuthWindow<'a> {
  pub fn new(server: &'a str, gp_params: &'a GpParams) -> Self {
    Self {
      server,
      gp_params,
      auth_request: None,
      clean: false,
      is_retrying: Default::default(),
    }
  }

  pub fn with_auth_request(mut self, auth_request: &'a str) -> Self {
    if !auth_request.is_empty() {
      self.auth_request = Some(auth_request);
    }
    self
  }

  pub fn with_clean(mut self, clean: bool) -> Self {
    self.clean = clean;
    self
  }

  pub async fn browser_authenticate(&self, browser: Option<&str>) -> anyhow::Result<SamlAuthData> {
    let auth_request = self.initial_auth_request().await?;
    let browser_auth = if let Some(browser) = browser {
      BrowserAuthenticator::new_with_browser(&auth_request, browser)
    } else {
      BrowserAuthenticator::new(&auth_request)
    };

    browser_auth.authenticate()?;
    info!("Please continue the authentication process in the default browser");

    wait_auth_data().await
  }

  pub async fn webview_authenticate(&self, app_handle: &AppHandle) -> anyhow::Result<SamlAuthData> {
    let auth_window = WebviewWindow::builder(app_handle, "auth_window", WebviewUrl::default())
      .title("GlobalProtect Login")
      .focused(true)
      .visible(false)
      .center()
      .build()?;

    self.auth_loop(&auth_window).await
  }

  async fn auth_loop(&self, auth_window: &WebviewWindow) -> anyhow::Result<SamlAuthData> {
    if self.clean {
      self.clear_webview_data(&auth_window).await?;
    }

    let auth_messenger = self.setup_auth_window(&auth_window).await?;

    loop {
      match auth_messenger.subscribe().await? {
        AuthEvent::Close => bail!("Authentication cancelled"),
        AuthEvent::RaiseWindow => self.raise_window(auth_window),
        AuthEvent::Error(AuthError::TlsError) => bail!(PortalError::TlsError),
        AuthEvent::Error(AuthError::NotFound) => self.handle_not_found(auth_window, &auth_messenger),
        AuthEvent::Error(AuthError::Invalid) => self.retry_auth(auth_window).await,
        AuthEvent::Data(auth_data) => {
          auth_window.close()?;
          return Ok(auth_data);
        }
      }
    }
  }

  async fn initial_auth_request(&self) -> anyhow::Result<Cow<'a, str>> {
    if let Some(auth_request) = self.auth_request {
      return Ok(Cow::Borrowed(auth_request));
    }

    let auth_request = portal_prelogin(&self.server, &self.gp_params).await?;
    Ok(Cow::Owned(auth_request))
  }

  async fn clear_webview_data(&self, auth_window: &WebviewWindow) -> anyhow::Result<()> {
    info!("Clearing webview data...");

    let (tx, rx) = oneshot::channel::<anyhow::Result<()>>();
    let now = Instant::now();
    auth_window.with_webview(|webview| {
      platform_impl::clear_data(&webview.inner(), |result| {
        if let Err(result) = tx.send(result) {
          warn!("Failed to send clear data result: {:?}", result);
        }
      })
    })?;

    rx.await??;
    info!("Webview data cleared in {:?}", now.elapsed());

    Ok(())
  }

  async fn setup_auth_window(&self, auth_window: &WebviewWindow) -> anyhow::Result<Arc<AuthMessenger>> {
    info!("Setting up auth window...");

    let auth_messenger = Arc::new(AuthMessenger::new());
    let auth_request = self.initial_auth_request().await?.into_owned();
    let ignore_tls_errors = self.gp_params.ignore_tls_errors();

    // Handle window close event
    let auth_messenger_clone = Arc::clone(&auth_messenger);
    auth_window.on_window_event(move |event| {
      if let WindowEvent::CloseRequested { .. } = event {
        auth_messenger_clone.send_auth_event(AuthEvent::Close);
      }
    });

    // Show the window after 10 seconds, so that the user can see the window if the auth process is stuck
    let auth_messenger_clone = Arc::clone(&auth_messenger);
    tokio::spawn(async move {
      time::sleep(Duration::from_secs(10)).await;
      auth_messenger_clone.send_auth_event(AuthEvent::RaiseWindow);
    });

    // setup webview
    let auth_messenger_clone = Arc::clone(&auth_messenger);
    let (tx, rx) = oneshot::channel::<anyhow::Result<()>>();

    auth_window.with_webview(move |webview| {
      let auth_settings = AuthSettings {
        auth_request: AuthRequest::new(&auth_request),
        auth_messenger: auth_messenger_clone,
        ignore_tls_errors,
      };

      let result = platform_impl::setup_webview(&webview.inner(), auth_settings);
      if let Err(result) = tx.send(result) {
        warn!("Failed to send setup auth window result: {:?}", result);
      }
    })?;

    rx.await??;
    info!("Auth window setup completed");

    Ok(auth_messenger)
  }

  fn handle_not_found(&self, auth_window: &WebviewWindow, auth_messenger: &Arc<AuthMessenger>) {
    info!("No auth data found, it may not be the /SAML20/SP/ACS endpoint");

    let visible = auth_window.is_visible().unwrap_or(false);
    if visible {
      return;
    }

    auth_messenger.schedule_raise_window(1);
  }

  async fn retry_auth(&self, auth_window: &WebviewWindow) {
    let mut is_retrying = self.is_retrying.write().await;
    if *is_retrying {
      info!("Already retrying authentication, skipping...");
      return;
    }

    *is_retrying = true;
    drop(is_retrying);

    if let Err(err) = self.retry_auth_impl(auth_window).await {
      warn!("Failed to retry authentication: {}", err);
    }

    *self.is_retrying.write().await = false;
  }

  async fn retry_auth_impl(&self, auth_window: &WebviewWindow) -> anyhow::Result<()> {
    info!("Retrying authentication...");

    auth_window.eval( r#"
      var loading = document.createElement("div");
      loading.innerHTML = '<div style="position: absolute; width: 100%; text-align: center; font-size: 20px; font-weight: bold; top: 50%; left: 50%; transform: translate(-50%, -50%);">Got invalid token, retrying...</div>';
      loading.style = "position: fixed; top: 0; left: 0; width: 100%; height: 100%; background: rgba(255, 255, 255, 0.85); z-index: 99999;";
      document.body.appendChild(loading);
    "#)?;

    let auth_request = portal_prelogin(&self.server, &self.gp_params).await?;
    let (tx, rx) = oneshot::channel::<()>();
    auth_window.with_webview(move |webview| {
      let auth_request = AuthRequest::new(&auth_request);
      platform_impl::load_auth_request(&webview.inner(), &auth_request);

      tx.send(()).expect("Failed to send message to the channel")
    })?;

    rx.await?;
    Ok(())
  }

  fn raise_window(&self, auth_window: &WebviewWindow) {
    let visible = auth_window.is_visible().unwrap_or(false);
    if visible {
      return;
    }

    info!("Raising auth window...");
    if let Err(err) = auth_window.raise() {
      warn!("Failed to raise window: {}", err);
    }
  }
}

async fn portal_prelogin(portal: &str, gp_params: &GpParams) -> anyhow::Result<String> {
  match prelogin(portal, gp_params).await? {
    Prelogin::Saml(prelogin) => Ok(prelogin.saml_request().to_string()),
    Prelogin::Standard(_) => bail!("Received non-SAML prelogin response"),
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
