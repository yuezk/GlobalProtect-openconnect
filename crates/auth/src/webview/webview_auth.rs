use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use anyhow::bail;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
use gpapi::utils::window::{WindowTypeHint, set_window_type_hint};
use gpapi::{
  auth::{AuthWindowTheme, AuthenticationCancelled, SamlAuthData},
  error::ServerTlsError,
  gp_params::GpParams,
  os_profile::{WebviewUserAgent, WebviewUserAgentTransform},
  utils::{redact::redact_uri, window::get_title_bar_height},
};
use log::{info, warn};
#[cfg(target_os = "macos")]
use tauri::Manager;
use tauri::{AppHandle, WebviewUrl, WebviewWindow, WindowEvent};
use tokio::{sync::oneshot, time};

use crate::auth_prelogin;

use super::address_bar::TOOLBAR_HEIGHT;
use super::auth_messenger::{AuthError, AuthEvent, AuthMessenger};
use super::recovery::{Recovery, RecoveryStatus, RetryPlan, RetrySource, RetryTiming};

const RESTART_TIMEOUT: Duration = Duration::from_secs(30);

pub trait PlatformWebviewExt {
  fn install_auth_controls(&self, window: &WebviewWindow, messenger: Arc<AuthMessenger>) -> anyhow::Result<()>;

  fn set_recovery_status(&self, status: RecoveryStatus) -> anyhow::Result<()>;

  fn stop_loading(&self);

  fn confirm_tls_exception(&self) -> anyhow::Result<()>;

  fn ignore_tls_errors(&self) -> anyhow::Result<()>;

  fn user_agent(&self) -> anyhow::Result<String>;

  fn set_user_agent(&self, user_agent: &str) -> anyhow::Result<()>;

  fn load_url(&self, url: &str) -> anyhow::Result<()>;

  fn load_html(&self, html: &str) -> anyhow::Result<()>;

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
  window_title: &'a str,
  window_theme: AuthWindowTheme,
}

fn native_theme(theme: AuthWindowTheme) -> Option<tauri::Theme> {
  match theme {
    AuthWindowTheme::System => None,
    AuthWindowTheme::Light => Some(tauri::Theme::Light),
    AuthWindowTheme::Dark => Some(tauri::Theme::Dark),
  }
}

pub fn apply_auth_theme(app_handle: &AppHandle, theme: AuthWindowTheme) {
  app_handle.set_theme(native_theme(theme));
}

fn present_after_setup(clean: bool) -> bool {
  clean
}

impl<'a> WebviewAuthenticator<'a> {
  pub fn new(server: &'a str, gp_params: &'a GpParams) -> Self {
    Self {
      server,
      gp_params,
      auth_request: None,
      webview_user_agent: None,
      clean: false,
      window_title: "GlobalProtect Login",
      window_theme: AuthWindowTheme::System,
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

  pub fn with_window_title(mut self, window_title: &'a str) -> Self {
    self.window_title = window_title;
    self
  }

  pub fn with_window_theme(mut self, window_theme: AuthWindowTheme) -> Self {
    self.window_theme = window_theme;
    self
  }

  pub async fn authenticate(&self, app_handle: &AppHandle) -> anyhow::Result<SamlAuthData> {
    let auth_messenger = Arc::new(AuthMessenger::new());
    let auth_window_builder = WebviewWindow::builder(app_handle, "auth_window", WebviewUrl::default())
      .title(self.window_title)
      .theme(native_theme(self.window_theme))
      .devtools(cfg!(any(debug_assertions, feature = "release-devtools")));
    let auth_window = auth_window_builder
      .inner_size(900.0, 650.0 + get_title_bar_height() + TOOLBAR_HEIGHT)
      .min_inner_size(400.0, 300.0)
      .focused(true)
      .visible(false)
      .center()
      .build()?;

    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    set_window_type_hint(&auth_window, WindowTypeHint::Dialog)?;

    let messenger = Arc::clone(&auth_messenger);
    let raise_task = tokio::spawn(async move {
      time::sleep(Duration::from_secs(10)).await;
      messenger.send_auth_event(AuthEvent::RaiseWindow);
    });
    let result = async {
      self
        .setup_auth_window(&auth_window, Arc::clone(&auth_messenger))
        .await?;
      if present_after_setup(self.clean) {
        self.raise_window(&auth_window);
      }
      self.wait_for_auth(&auth_window, Arc::clone(&auth_messenger)).await
    }
    .await;

    raise_task.abort();
    auth_messenger.begin_attempt();
    auth_messenger.cancel_raise_window();
    // Preserve the authentication outcome even if native teardown fails.
    if let Err(error) = auth_window.close() {
      warn!("Failed to close authentication window: {error}");
    }
    result
  }

  async fn wait_for_auth(
    &self,
    auth_window: &WebviewWindow,
    auth_messenger: Arc<AuthMessenger>,
  ) -> anyhow::Result<SamlAuthData> {
    let mut recovery = Recovery::default();
    let mut restart: Option<Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + '_>>> = None;
    let mut loading_deadline = None;

    loop {
      let event = tokio::select! {
        event = auth_messenger.subscribe() => event?,
        result = async {
          match restart.as_mut() {
            Some(restart) => restart.await,
            None => std::future::pending().await,
          }
        } => {
          restart = None;
          match result {
            Ok(auth_request) => {
              // Fence off reads and cancellation notifications from the old page.
              auth_messenger.begin_attempt();
              recovery.loading();
              loading_deadline = Some(time::Instant::now() + RESTART_TIMEOUT);
              self.load_restarted_sign_in(auth_window, &auth_messenger, auth_request)?;
            }
            Err(err) => {
              warn!("Failed to restart sign-in: {}", err);
              if err.downcast_ref::<ServerTlsError>().is_some() {
                return Err(err);
              }
              recovery.load_failed();
            }
          }
          self.update_recovery(auth_window, recovery.status())?;
          continue;
        }
        _ = async {
          match loading_deadline {
            Some(deadline) => time::sleep_until(deadline).await,
            None => std::future::pending().await,
          }
        } => {
          loading_deadline = None;
          recovery.load_failed();
          self.update_recovery(auth_window, recovery.status())?;
          continue;
        }
      };
      let mut retry_plan = None;
      match event {
        AuthEvent::Close => {
          bail!(AuthenticationCancelled);
        }
        AuthEvent::RaiseWindow => self.raise_window(auth_window),
        AuthEvent::Data(generation, ..)
        | AuthEvent::Error(generation, _)
        | AuthEvent::NoAuthData(generation, _)
        | AuthEvent::PageLoaded(generation)
        | AuthEvent::TlsAccepted(generation)
          if generation != auth_messenger.generation() =>
        {
          continue;
        }
        AuthEvent::Error(_, AuthError::Tls(failure)) => {
          warn!("TLS failure for {}: {}", failure.host, failure.reason.message());
          restart = None;
          loading_deadline = None;
          recovery.tls_failed(failure.reason, failure.can_continue);
        }
        AuthEvent::RequestTlsException => {
          if recovery.status().can_continue() {
            auth_window.with_webview(|wv| {
              if let Err(err) = wv.confirm_tls_exception() {
                warn!("Failed to confirm certificate exception: {}", err);
              }
            })?;
          }
        }
        AuthEvent::TlsAccepted(_) => {
          retry_plan = recovery.certificate_accepted();
        }
        AuthEvent::NoAuthData(_, location) => {
          info!(
            "No auth data found in {:?}, it may not be the /SAML20/SP/ACS endpoint",
            location
          );
          self.handle_not_found(auth_window, &auth_messenger);
        }
        AuthEvent::Error(_, AuthError::Invalid(err, location)) => {
          warn!("Got invalid auth data in {:?}: {}", location, err);
          recovery.failure(false);
        }
        AuthEvent::Error(_, AuthError::TemporarilyUnavailable) => {
          retry_plan = recovery.failure(true);
        }
        AuthEvent::Error(_, AuthError::LoadFailed(err)) => {
          if matches!(
            recovery.status(),
            RecoveryStatus::Loading | RecoveryStatus::Hidden | RecoveryStatus::CertificateBypassed
          ) {
            warn!("Failed to load sign-in: {}", err);
            recovery.load_failed();
            loading_deadline = None;
          }
        }
        AuthEvent::Retry => {
          retry_plan = recovery.retry();
        }
        AuthEvent::PageLoaded(_) => {
          recovery.page_loaded();
          if !recovery.status().blocks_page() {
            loading_deadline = None;
          }
        }
        AuthEvent::Data(_, auth_data, location) => {
          if !recovery.status().accepts_auth_data() {
            continue;
          }
          info!("Got auth data from {:?}", location);

          return Ok(auth_data);
        }
      }
      if let Some(plan) = retry_plan {
        loading_deadline = None;
        auth_messenger.begin_attempt();
        auth_window.with_webview(|wv| wv.stop_loading())?;
        restart = Some(Box::pin(async move {
          time::timeout(RESTART_TIMEOUT, self.request_sign_in(plan)).await?
        }));
      }
      if recovery.status() != RecoveryStatus::Loading {
        loading_deadline = None;
      }
      self.update_recovery(auth_window, recovery.status())?;
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

    let auth_request = match self.auth_request {
      Some(auth_request) => auth_request.to_string(),
      None => auth_prelogin(&self.server, &self.gp_params, false, false).await?,
    };

    let (tx, rx) = oneshot::channel::<anyhow::Result<()>>();
    let ignore_tls_errors = self.gp_params.ignore_tls_errors();
    let webview_user_agent = self.webview_user_agent.clone();
    let window = auth_window.clone();

    // Set up webview
    auth_window.with_webview(move |wv| {
      let controls_messenger = Arc::clone(&auth_messenger);
      let result = || -> anyhow::Result<()> {
        wv.install_auth_controls(&window, controls_messenger)?;
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

  fn update_recovery(&self, auth_window: &WebviewWindow, status: RecoveryStatus) -> anyhow::Result<()> {
    if status != RecoveryStatus::Hidden {
      self.raise_window(auth_window);
    }
    auth_window.with_webview(move |wv| {
      if let Err(err) = wv.set_recovery_status(status) {
        warn!("Failed to update sign-in status: {}", err);
      }
    })?;
    Ok(())
  }

  async fn request_sign_in(&self, plan: RetryPlan) -> anyhow::Result<String> {
    info!("Restarting sign-in...");
    if plan.timing == RetryTiming::AutomaticDelay {
      time::sleep(Duration::from_secs(1)).await;
    }
    if plan.source == RetrySource::SuppliedRequest {
      if let Some(request) = self.auth_request {
        return Ok(request.to_owned());
      }
    }
    auth_prelogin(self.server, self.gp_params, false, false).await
  }

  fn load_restarted_sign_in(
    &self,
    auth_window: &WebviewWindow,
    messenger: &Arc<AuthMessenger>,
    auth_request: String,
  ) -> anyhow::Result<()> {
    let webview_user_agent = self.webview_user_agent.clone();
    let generation = messenger.generation();
    let messenger = Arc::clone(messenger);
    auth_window.with_webview(move |wv| {
      if generation != messenger.generation() {
        return;
      }
      if let Err(err) = load_auth_request_with_user_agent(&wv, webview_user_agent.as_ref(), &auth_request) {
        messenger.send_auth_event(AuthEvent::Error(generation, AuthError::LoadFailed(err)));
      }
    })?;
    Ok(())
  }

  fn raise_window(&self, auth_window: &WebviewWindow) {
    let visible = auth_window.is_visible().unwrap_or(false);
    if visible {
      return;
    }

    info!("Raising auth window...");

    #[cfg(target_os = "macos")]
    if let Err(err) = auth_window
      .app_handle()
      .set_activation_policy(tauri::ActivationPolicy::Regular)
    {
      warn!("Failed to show the application in the Dock: {}", err);
    }

    use gpapi::utils::window::WindowExt;
    let result = auth_window.raise();

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
  fn maps_auth_window_theme_to_native_theme() {
    assert_eq!(native_theme(AuthWindowTheme::System), None);
    assert_eq!(native_theme(AuthWindowTheme::Light), Some(tauri::Theme::Light));
    assert_eq!(native_theme(AuthWindowTheme::Dark), Some(tauri::Theme::Dark));
  }

  #[test]
  fn presents_clean_auth_after_setup() {
    assert!(present_after_setup(true));
    assert!(!present_after_setup(false));
  }

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
