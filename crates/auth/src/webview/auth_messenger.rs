use anyhow::bail;
use gpapi::{auth::SamlAuthData, error::AuthDataParseError};
use log::{error, info};
use regex::Regex;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub(crate) enum AuthDataLocation {
  #[cfg(not(target_os = "macos"))]
  Headers,
  Body,
}

#[derive(Debug)]
pub(crate) enum AuthError {
  /// Failed to load page due to TLS error
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  TlsError,
  /// 1. Found auth data in headers/body but it's invalid
  /// 2. Loaded an empty page, failed to load page. etc.
  Invalid(anyhow::Error, AuthDataLocation),
  /// No auth data found in headers/body
  NotFound(AuthDataLocation),
}

impl AuthError {
  pub fn invalid_from_body(err: anyhow::Error) -> Self {
    Self::Invalid(err, AuthDataLocation::Body)
  }

  pub fn not_found_in_body() -> Self {
    Self::NotFound(AuthDataLocation::Body)
  }
}

#[cfg(not(target_os = "macos"))]
impl AuthError {
  pub fn not_found_in_headers() -> Self {
    Self::NotFound(AuthDataLocation::Headers)
  }
}

pub(crate) enum AuthEvent {
  Data(SamlAuthData, AuthDataLocation),
  Error(AuthError),
  RaiseWindow,
  Close,
}

pub struct AuthMessenger {
  tx: mpsc::UnboundedSender<AuthEvent>,
  rx: RwLock<mpsc::UnboundedReceiver<AuthEvent>>,
  raise_window_cancel_token: RwLock<Option<CancellationToken>>,
}

impl AuthMessenger {
  pub fn new() -> Self {
    let (tx, rx) = mpsc::unbounded_channel();

    Self {
      tx,
      rx: RwLock::new(rx),
      raise_window_cancel_token: Default::default(),
    }
  }

  pub async fn subscribe(&self) -> anyhow::Result<AuthEvent> {
    let mut rx = self.rx.write().await;
    if let Some(event) = rx.recv().await {
      return Ok(event);
    }
    bail!("Failed to receive auth event");
  }

  pub fn send_auth_event(&self, event: AuthEvent) {
    if let Err(event) = self.tx.send(event) {
      error!("Failed to send auth event: {}", event);
    }
  }

  pub fn send_auth_error(&self, err: AuthError) {
    self.send_auth_event(AuthEvent::Error(err));
  }

  fn send_auth_data(&self, data: SamlAuthData, location: AuthDataLocation) {
    self.send_auth_event(AuthEvent::Data(data, location));
  }

  pub fn schedule_raise_window(&self, delay: u64) {
    let Ok(mut guard) = self.raise_window_cancel_token.try_write() else {
      return;
    };

    // Return if the previous raise window task is still running
    if let Some(token) = guard.as_ref() {
      if !token.is_cancelled() {
        info!("Raise window task is still running, skipping...");
        return;
      }
    }

    let cancel_token = CancellationToken::new();
    let cancel_token_clone = cancel_token.clone();

    *guard = Some(cancel_token_clone);

    let tx = self.tx.clone();
    tokio::spawn(async move {
      info!("Displaying the window in {} second(s)...", delay);

      tokio::select! {
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(delay)) => {
          cancel_token.cancel();

          if let Err(err) = tx.send(AuthEvent::RaiseWindow) {
            error!("Failed to send raise window event: {}", err);
          }
        }
        _ = cancel_token.cancelled() => {
          info!("Cancelled raise window task");
        }
      }
    });
  }

  pub fn cancel_raise_window(&self) {
    if let Ok(mut cancel_token) = self.raise_window_cancel_token.try_write() {
      if let Some(token) = cancel_token.take() {
        token.cancel();
      }
    }
  }

  pub fn read_from_html(&self, html: &str) {
    if html.contains("Temporarily Unavailable") {
      return self.send_auth_error(AuthError::invalid_from_body(anyhow::anyhow!("Temporarily Unavailable")));
    }

    let auth_result = SamlAuthData::from_html(html).or_else(|err| {
      info!("Read auth data from html failed: {}, extracting gpcallback...", err);

      if let Some(gpcallback) = extract_gpcallback(html) {
        info!("Found gpcallback from html...");
        SamlAuthData::from_gpcallback(&gpcallback)
      } else {
        Err(err)
      }
    });

    match auth_result {
      Ok(data) => self.send_auth_data(data, AuthDataLocation::Body),
      Err(AuthDataParseError::Invalid(err)) => self.send_auth_error(AuthError::invalid_from_body(err)),
      Err(AuthDataParseError::NotFound) => self.send_auth_error(AuthError::not_found_in_body()),
    }
  }

  #[cfg(not(target_os = "macos"))]
  pub fn read_from_response(&self, auth_response: &impl super::webview_auth::GetHeader) {
    use log::warn;

    let Some(status) = auth_response.get_header("saml-auth-status") else {
      return self.send_auth_error(AuthError::not_found_in_headers());
    };

    // Do not send auth error when reading from headers, as the html body may contain the auth data
    if status != "1" {
      warn!("Found invalid saml-auth-status in headers: {}", status);
      return;
    }

    let username = auth_response.get_header("saml-username");
    let prelogin_cookie = auth_response.get_header("prelogin-cookie");
    let portal_userauthcookie = auth_response.get_header("portal-userauthcookie");

    match SamlAuthData::new(username, prelogin_cookie, portal_userauthcookie) {
      Ok(auth_data) => self.send_auth_data(auth_data, AuthDataLocation::Headers),
      Err(err) => {
        warn!("Failed to read auth data from headers: {}", err);
      }
    }
  }
}

fn extract_gpcallback(html: &str) -> Option<String> {
  let re = Regex::new(r#"globalprotectcallback:[^"]+"#).unwrap();
  re.captures(html)
    .and_then(|captures| captures.get(0))
    .map(|m| html_escape::decode_html_entities(m.as_str()).to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn extract_gpcallback_some() {
    let html = r#"
      <meta http-equiv="refresh" content="0; URL=globalprotectcallback:PGh0bWw+PCEtLSA8c">
      <meta http-equiv="refresh" content="0; URL=globalprotectcallback:PGh0bWw+PCEtLSA8c">
    "#;

    assert_eq!(
      extract_gpcallback(html).as_deref(),
      Some("globalprotectcallback:PGh0bWw+PCEtLSA8c")
    );
  }

  #[test]
  fn extract_gpcallback_cas() {
    let html = r#"
      <meta http-equiv="refresh" content="0; URL=globalprotectcallback:cas-as=1&amp;un=xyz@email.com&amp;token=very_long_string">
    "#;

    assert_eq!(
      extract_gpcallback(html).as_deref(),
      Some("globalprotectcallback:cas-as=1&un=xyz@email.com&token=very_long_string")
    );
  }

  #[test]
  fn extract_gpcallback_none() {
    let html = r#"
      <meta http-equiv="refresh" content="0; URL=PGh0bWw+PCEtLSA8c">
    "#;

    assert_eq!(extract_gpcallback(html), None);
  }
}
