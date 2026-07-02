use std::{sync::Arc, time::Duration};

use anyhow::bail;
use gpapi::{
  auth::{AuthenticationCancelled, SamlAuthData},
  gp_params::GpParams,
  os_profile::{WebviewUserAgent, WebviewUserAgentTransform},
  utils::redact::redact_uri,
};
use log::{info, warn};
use tauri::{
  AppHandle, WebviewUrl, WebviewWindow, WindowEvent,
  webview::{PageLoadEvent, PageLoadPayload},
};
use tokio::{sync::oneshot, time};

use crate::auth_prelogin;

use super::auth_messenger::{AuthError, AuthEvent, AuthMessenger};

pub trait PlatformWebviewExt {
  fn ignore_tls_errors(&self) -> anyhow::Result<()>;

  fn user_agent(&self) -> anyhow::Result<String>;

  fn set_user_agent(&self, user_agent: &str) -> anyhow::Result<()>;

  fn load_url(&self, url: &str) -> anyhow::Result<()>;

  fn load_html(&self, html: &str) -> anyhow::Result<()>;

  fn get_html(&self, callback: Box<dyn Fn(anyhow::Result<String>) + 'static>);

  fn load_auth_request(&self, auth_request: &str) -> anyhow::Result<()> {
    if auth_request.starts_with("http") {
      info!("Loading auth request as URL: {}", redact_uri(auth_request));
      self.load_url(auth_request)
    } else {
      info!("Loading auth request as HTML...");
      self.load_html(auth_request)
    }
  }
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub trait GetHeader {
  fn get_header(&self, key: &str) -> Option<String>;
}

pub struct WebviewAuthenticator<'a> {
  server: &'a str,
  gp_params: &'a GpParams,
  auth_request: Option<&'a str>,
  webview_user_agent: Option<WebviewUserAgent>,
  clean: bool,

  is_retrying: tokio::sync::RwLock<bool>,
}

impl<'a> WebviewAuthenticator<'a> {
  pub fn new(server: &'a str, gp_params: &'a GpParams) -> Self {
    Self {
      server,
      gp_params,
      auth_request: None,
      webview_user_agent: None,
      clean: false,
      is_retrying: Default::default(),
    }
  }

  pub fn with_auth_request(mut self, auth_request: &'a str) -> Self {
    self.auth_request = Some(auth_request);
    self
  }

  pub fn with_webview_user_agent(mut self, user_agent: WebviewUserAgent) -> Self {
    self.webview_user_agent = Some(user_agent);
    self
  }

  pub fn with_clean(mut self, clean: bool) -> Self {
    self.clean = clean;
    self
  }

  pub async fn authenticate(&self, app_handle: &AppHandle) -> anyhow::Result<SamlAuthData> {
    let auth_messenger = Arc::new(AuthMessenger::new());
    let auth_messenger_clone = Arc::clone(&auth_messenger);

    let on_page_load = move |auth_window: WebviewWindow, event: PageLoadPayload<'_>| {
      let auth_messenger_clone = Arc::clone(&auth_messenger_clone);
      let redacted_url = redact_uri(event.url().as_str());

      match event.event() {
        PageLoadEvent::Started => {
          info!("Started loading page: {}", redacted_url);
          auth_messenger_clone.cancel_raise_window();
        }
        PageLoadEvent::Finished => {
          info!("Finished loading page: {}", redacted_url);
        }
      }

      // Read auth data from the page no matter whether it's finished loading or not
      // Because we found that the finished event may not be triggered in some cases (e.g., on macOS)
      let _ = auth_window.with_webview(move |wv| {
        wv.get_html(Box::new(move |html| match html {
          Ok(html) => auth_messenger_clone.read_from_html(&html),
          Err(err) => warn!("Failed to get html: {}", err),
        }));
      });
    };

    let title_bar_height = if cfg!(target_os = "macos") { 28.0 } else { 0.0 };

    let auth_window = WebviewWindow::builder(app_handle, "auth_window", WebviewUrl::default())
      .on_page_load(on_page_load)
      .title("GlobalProtect Login")
      .inner_size(900.0, 650.0 + title_bar_height)
      .focused(true)
      // when clean is true, the window is expected to be shown because the cookies are cleared
      .visible(self.clean)
      .center()
      .build()?;

    self
      .setup_auth_window(&auth_window, Arc::clone(&auth_messenger))
      .await?;

    loop {
      match auth_messenger.subscribe().await? {
        AuthEvent::Close => bail!(AuthenticationCancelled),
        AuthEvent::RaiseWindow => self.raise_window(&auth_window),
        #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
        AuthEvent::Error(AuthError::TlsError) => bail!(gpapi::error::PortalError::TlsError),
        AuthEvent::Error(AuthError::NotFound(location)) => {
          info!(
            "No auth data found in {:?}, it may not be the /SAML20/SP/ACS endpoint",
            location
          );
          self.handle_not_found(&auth_window, &auth_messenger);
        }
        AuthEvent::Error(AuthError::Invalid(err, location)) => {
          warn!("Got invalid auth data in {:?}: {}", location, err);
          self.retry_auth(&auth_window).await;
        }
        AuthEvent::Data(auth_data, location) => {
          info!("Got auth data from {:?}", location);

          auth_window.close()?;
          return Ok(auth_data);
        }
      }
    }
  }

  async fn setup_auth_window(
    &self,
    auth_window: &WebviewWindow,
    auth_messenger: Arc<AuthMessenger>,
  ) -> anyhow::Result<()> {
    info!("Setting up auth window...");

    if self.clean {
      info!("Clearing all browsing data...");
      auth_window.clear_all_browsing_data()?;
    }

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

    let auth_request = match self.auth_request {
      Some(auth_request) => auth_request.to_string(),
      None => auth_prelogin(&self.server, &self.gp_params, false, false).await?,
    };

    let (tx, rx) = oneshot::channel::<anyhow::Result<()>>();
    let ignore_tls_errors = self.gp_params.ignore_tls_errors();
    let webview_user_agent = self.webview_user_agent.clone();

    // Set up webview
    auth_window.with_webview(move |wv| {
      #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
      {
        use super::platform_impl::PlatformWebviewOnResponse;
        wv.on_response(Box::new(move |response| match response {
          Ok(response) => auth_messenger.read_from_response(&response),
          Err(err) => auth_messenger.send_auth_error(err),
        }));
      }

      let result = || -> anyhow::Result<()> {
        if ignore_tls_errors {
          wv.ignore_tls_errors()?;
        }

        load_auth_request_with_user_agent(&wv, webview_user_agent.as_ref(), &auth_request)
      }();

      if let Err(result) = tx.send(result) {
        warn!("Failed to send setup auth window result: {:?}", result);
      }
    })?;

    rx.await??;
    info!("Auth window setup completed");

    Ok(())
  }

  fn handle_not_found(&self, auth_window: &WebviewWindow, auth_messenger: &Arc<AuthMessenger>) {
    let visible = auth_window.is_visible().unwrap_or(false);
    if visible {
      return;
    }

    auth_messenger.schedule_raise_window(2);
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

    let auth_request = auth_prelogin(&self.server, &self.gp_params, false, false).await?;

    let (tx, rx) = oneshot::channel::<anyhow::Result<()>>();
    let webview_user_agent = self.webview_user_agent.clone();
    auth_window.with_webview(move |wv| {
      let result = || -> anyhow::Result<()> {
        load_auth_request_with_user_agent(&wv, webview_user_agent.as_ref(), &auth_request)
      }();

      if let Err(result) = tx.send(result) {
        warn!("Failed to send retry auth result: {:?}", result);
      }
    })?;

    rx.await??;

    Ok(())
  }

  fn raise_window(&self, auth_window: &WebviewWindow) {
    let visible = auth_window.is_visible().unwrap_or(false);
    if visible {
      return;
    }

    info!("Raising auth window...");

    #[cfg(target_os = "macos")]
    let result = auth_window.show();

    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    let result = {
      use gpapi::utils::window::WindowExt;
      auth_window.raise()
    };

    if let Err(err) = result {
      warn!("Failed to raise window: {}", err);
    }
  }
}

fn load_auth_request_with_user_agent(
  wv: &impl PlatformWebviewExt,
  plan: Option<&WebviewUserAgent>,
  auth_request: &str,
) -> anyhow::Result<()> {
  if let Some(plan) = plan {
    let user_agent = compose_webview_user_agent(plan, || wv.user_agent())?;
    wv.set_user_agent(&user_agent)?;
  }

  wv.load_auth_request(auth_request)
}

fn compose_webview_user_agent(
  plan: &WebviewUserAgent,
  native_user_agent: impl FnOnce() -> anyhow::Result<String>,
) -> anyhow::Result<String> {
  let (prefix, default_user_agent, transform) = match plan {
    WebviewUserAgent::Native { prefix, transform } => (prefix.as_str(), native_user_agent()?, *transform),
    WebviewUserAgent::Projected {
      prefix,
      default_user_agent,
    } => (
      prefix.as_str(),
      default_user_agent.clone(),
      WebviewUserAgentTransform::None,
    ),
  };

  let default_user_agent = apply_webview_user_agent_transform(transform, &default_user_agent);
  prepend_webview_user_agent_prefix(prefix, &default_user_agent)
}

fn apply_webview_user_agent_transform(transform: WebviewUserAgentTransform, user_agent: &str) -> String {
  match transform {
    WebviewUserAgentTransform::None => user_agent.to_string(),
    WebviewUserAgentTransform::LinuxPanGpuiVersion => linux_pan_gpui_user_agent(user_agent),
  }
}

fn linux_pan_gpui_user_agent(user_agent: &str) -> String {
  if user_agent.contains("PanGPUI Version/10.0") {
    return user_agent.to_string();
  }

  let Some(start) = user_agent.find("Version/") else {
    return user_agent.to_string();
  };
  let end = user_agent[start..]
    .find(char::is_whitespace)
    .map(|offset| start + offset)
    .unwrap_or(user_agent.len());

  format!("{}PanGPUI Version/10.0{}", &user_agent[..start], &user_agent[end..])
}

fn prepend_webview_user_agent_prefix(prefix: &str, default_user_agent: &str) -> anyhow::Result<String> {
  let prefix = prefix.trim();
  if prefix.is_empty() {
    return Ok(default_user_agent.to_string());
  }

  let default_user_agent = default_user_agent.trim();
  if default_user_agent.is_empty() {
    bail!("Failed to read default webview user agent");
  }

  if default_user_agent.starts_with(prefix) {
    return Ok(default_user_agent.to_string());
  }

  Ok(format!("{} {}", prefix, default_user_agent))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn prepends_prefix_to_default_user_agent() {
    let user_agent = prepend_webview_user_agent_prefix("PAN GlobalProtect/6.3.3", "Mozilla/5.0").unwrap();

    assert_eq!(user_agent, "PAN GlobalProtect/6.3.3 Mozilla/5.0");
  }

  #[test]
  fn leaves_already_prefixed_user_agent_unchanged() {
    let user_agent =
      prepend_webview_user_agent_prefix("PAN GlobalProtect/6.3.3", "PAN GlobalProtect/6.3.3 Mozilla/5.0").unwrap();

    assert_eq!(user_agent, "PAN GlobalProtect/6.3.3 Mozilla/5.0");
  }

  #[test]
  fn blank_prefix_leaves_default_user_agent_unchanged() {
    let user_agent = prepend_webview_user_agent_prefix(" ", "Mozilla/5.0").unwrap();

    assert_eq!(user_agent, "Mozilla/5.0");
  }

  #[test]
  fn non_blank_prefix_requires_default_user_agent() {
    let err = prepend_webview_user_agent_prefix("PAN GlobalProtect/6.3.3", " ").unwrap_err();

    assert!(err.to_string().contains("default webview user agent"));
  }

  #[test]
  fn native_plan_reads_native_default_user_agent() {
    let plan = WebviewUserAgent::Native {
      prefix: "PAN GlobalProtect/6.3.3".to_string(),
      transform: WebviewUserAgentTransform::None,
    };
    let user_agent = compose_webview_user_agent(&plan, || Ok("Mozilla/5.0".to_string())).unwrap();

    assert_eq!(user_agent, "PAN GlobalProtect/6.3.3 Mozilla/5.0");
  }

  #[test]
  fn linux_transform_replaces_version_token_with_pan_gpui_version() {
    let user_agent = linux_pan_gpui_user_agent(
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/60.5 Safari/605.1.15",
    );

    assert_eq!(
      user_agent,
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) PanGPUI Version/10.0 Safari/605.1.15"
    );
  }

  #[test]
  fn linux_transform_does_not_duplicate_pan_gpui_version() {
    let user_agent = linux_pan_gpui_user_agent(
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/602.1 (KHTML, like Gecko) PanGPUI Version/10.0 Safari/602.1",
    );

    assert_eq!(
      user_agent,
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/602.1 (KHTML, like Gecko) PanGPUI Version/10.0 Safari/602.1"
    );
  }

  #[test]
  fn native_linux_plan_applies_pan_gpui_transform_before_prefix() {
    let plan = WebviewUserAgent::Native {
      prefix: "PAN GlobalProtect/6.3.3".to_string(),
      transform: WebviewUserAgentTransform::LinuxPanGpuiVersion,
    };
    let user_agent =
      compose_webview_user_agent(&plan, || Ok("Mozilla/5.0 Version/60.5 Safari/605.1.15".to_string())).unwrap();

    assert_eq!(
      user_agent,
      "PAN GlobalProtect/6.3.3 Mozilla/5.0 PanGPUI Version/10.0 Safari/605.1.15"
    );
  }

  #[test]
  fn projected_plan_uses_profile_default_user_agent() {
    let plan = WebviewUserAgent::Projected {
      prefix: "PAN GlobalProtect/6.3.3".to_string(),
      default_user_agent: "Mozilla/5.0 projected".to_string(),
    };
    let user_agent = compose_webview_user_agent(&plan, || {
      panic!("projected webview user agent should not read the native default")
    })
    .unwrap();

    assert_eq!(user_agent, "PAN GlobalProtect/6.3.3 Mozilla/5.0 projected");
  }
}
