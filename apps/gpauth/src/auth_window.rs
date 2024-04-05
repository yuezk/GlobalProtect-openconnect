use std::{
  rc::Rc,
  sync::Arc,
  time::{Duration, Instant},
};

use anyhow::bail;
use gpapi::{
  auth::SamlAuthData,
  error::AuthDataParseError,
  gp_params::GpParams,
  portal::{prelogin, Prelogin},
  utils::{redact::redact_uri, window::WindowExt},
};
use log::{info, warn};
use regex::Regex;
use tauri::{AppHandle, Window, WindowEvent, WindowUrl};
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio_util::sync::CancellationToken;
use webkit2gtk::{
  gio::Cancellable,
  glib::{GString, TimeSpan},
  LoadEvent, SettingsExt, TLSErrorsPolicy, URIResponse, URIResponseExt, WebContextExt, WebResource, WebResourceExt,
  WebView, WebViewExt, WebsiteDataManagerExtManual, WebsiteDataTypes,
};

enum AuthDataError {
  /// Failed to load page due to TLS error
  TlsError,
  /// 1. Found auth data in headers/body but it's invalid
  /// 2. Loaded an empty page, failed to load page. etc.
  Invalid,
  /// No auth data found in headers/body
  NotFound,
}

type AuthResult = Result<SamlAuthData, AuthDataError>;

pub(crate) struct AuthWindow<'a> {
  app_handle: AppHandle,
  server: &'a str,
  saml_request: &'a str,
  user_agent: &'a str,
  gp_params: Option<GpParams>,
  clean: bool,
}

impl<'a> AuthWindow<'a> {
  pub fn new(app_handle: AppHandle) -> Self {
    Self {
      app_handle,
      server: "",
      saml_request: "",
      user_agent: "",
      gp_params: None,
      clean: false,
    }
  }

  pub fn server(mut self, server: &'a str) -> Self {
    self.server = server;
    self
  }

  pub fn saml_request(mut self, saml_request: &'a str) -> Self {
    self.saml_request = saml_request;
    self
  }

  pub fn user_agent(mut self, user_agent: &'a str) -> Self {
    self.user_agent = user_agent;
    self
  }

  pub fn gp_params(mut self, gp_params: GpParams) -> Self {
    self.gp_params.replace(gp_params);
    self
  }

  pub fn clean(mut self, clean: bool) -> Self {
    self.clean = clean;
    self
  }

  pub async fn open(&self) -> anyhow::Result<SamlAuthData> {
    info!("Open auth window, user_agent: {}", self.user_agent);

    let window = Window::builder(&self.app_handle, "auth_window", WindowUrl::default())
      .title("GlobalProtect Login")
      // .user_agent(self.user_agent)
      .focused(true)
      .visible(false)
      .center()
      .build()?;

    let window = Arc::new(window);

    let cancel_token = CancellationToken::new();
    let cancel_token_clone = cancel_token.clone();

    window.on_window_event(move |event| {
      if let WindowEvent::CloseRequested { .. } = event {
        cancel_token_clone.cancel();
      }
    });

    let window_clone = Arc::clone(&window);
    let timeout_secs = 15;
    tokio::spawn(async move {
      tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
      let visible = window_clone.is_visible().unwrap_or(false);
      if !visible {
        info!("Try to raise auth window after {} seconds", timeout_secs);
        raise_window(&window_clone);
      }
    });

    tokio::select! {
      _ = cancel_token.cancelled() => {
        bail!("Auth cancelled");
      }
      saml_result = self.auth_loop(&window) => {
        window.close()?;
        saml_result
      }
    }
  }

  async fn auth_loop(&self, window: &Arc<Window>) -> anyhow::Result<SamlAuthData> {
    let saml_request = self.saml_request.to_string();
    let (auth_result_tx, mut auth_result_rx) = mpsc::unbounded_channel::<AuthResult>();
    let raise_window_cancel_token: Arc<RwLock<Option<CancellationToken>>> = Default::default();
    let gp_params = self.gp_params.as_ref().unwrap();
    let tls_err_policy = if gp_params.ignore_tls_errors() {
      TLSErrorsPolicy::Ignore
    } else {
      TLSErrorsPolicy::Fail
    };

    if self.clean {
      clear_webview_cookies(window).await?;
    }

    let raise_window_cancel_token_clone = Arc::clone(&raise_window_cancel_token);
    window.with_webview(move |wv| {
      let wv = wv.inner();

      if let Some(context) = wv.context() {
        context.set_tls_errors_policy(tls_err_policy);
      }

      if let Some(settings) = wv.settings() {
        let ua = settings.user_agent().unwrap_or("".into());
        info!("Auth window user agent: {}", ua);
      }

      // Load the initial SAML request
      load_saml_request(&wv, &saml_request);

      let auth_result_tx_clone = auth_result_tx.clone();
      wv.connect_load_changed(move |wv, event| {
        if event == LoadEvent::Started {
          let Ok(mut cancel_token) = raise_window_cancel_token_clone.try_write() else {
            return;
          };

          // Cancel the raise window task
          if let Some(cancel_token) = cancel_token.take() {
            cancel_token.cancel();
          }
          return;
        }

        if event != LoadEvent::Finished {
          return;
        }

        if let Some(main_resource) = wv.main_resource() {
          let uri = main_resource.uri().unwrap_or("".into());

          if uri.is_empty() {
            warn!("Loaded an empty uri");
            send_auth_result(&auth_result_tx_clone, Err(AuthDataError::Invalid));
            return;
          }

          info!("Loaded uri: {}", redact_uri(&uri));
          if uri.starts_with("globalprotectcallback:") {
            return;
          }

          read_auth_data(&main_resource, auth_result_tx_clone.clone());
        }
      });

      let auth_result_tx_clone = auth_result_tx.clone();
      wv.connect_load_failed_with_tls_errors(move |_wv, uri, cert, err| {
        let redacted_uri = redact_uri(uri);
        warn!(
          "Failed to load uri: {} with error: {}, cert: {}",
          redacted_uri, err, cert
        );

        send_auth_result(&auth_result_tx_clone, Err(AuthDataError::TlsError));
        true
      });

      wv.connect_load_failed(move |_wv, _event, uri, err| {
        let redacted_uri = redact_uri(uri);
        if !uri.starts_with("globalprotectcallback:") {
          warn!("Failed to load uri: {} with error: {}", redacted_uri, err);
        }
        // NOTE: Don't send error here, since load_changed event will be triggered after this
        // send_auth_result(&auth_result_tx, Err(AuthDataError::Invalid));
        // true to stop other handlers from being invoked for the event. false to propagate the event further.
        true
      });
    })?;

    let portal = self.server.to_string();

    loop {
      if let Some(auth_result) = auth_result_rx.recv().await {
        match auth_result {
          Ok(auth_data) => return Ok(auth_data),
          Err(AuthDataError::TlsError) => bail!("TLS error: certificate verify failed"),
          Err(AuthDataError::NotFound) => {
            info!("No auth data found, it may not be the /SAML20/SP/ACS endpoint");

            // The user may need to interact with the auth window, raise it in 3 seconds
            if !window.is_visible().unwrap_or(false) {
              let window = Arc::clone(window);
              let cancel_token = CancellationToken::new();

              raise_window_cancel_token.write().await.replace(cancel_token.clone());

              tokio::spawn(async move {
                let delay_secs = 1;

                info!("Raise window in {} second(s)", delay_secs);
                tokio::select! {
                  _ = tokio::time::sleep(Duration::from_secs(delay_secs)) => {
                    raise_window(&window);
                  }
                  _ = cancel_token.cancelled() => {
                    info!("Raise window cancelled");
                  }
                }
              });
            }
          }
          Err(AuthDataError::Invalid) => {
            info!("Got invalid auth data, retrying...");

            window.with_webview(|wv| {
              let wv = wv.inner();
              wv.run_javascript(r#"
                  var loading = document.createElement("div");
                  loading.innerHTML = '<div style="position: absolute; width: 100%; text-align: center; font-size: 20px; font-weight: bold; top: 50%; left: 50%; transform: translate(-50%, -50%);">Got invalid token, retrying...</div>';
                  loading.style = "position: fixed; top: 0; left: 0; width: 100%; height: 100%; background: rgba(255, 255, 255, 0.85); z-index: 99999;";
                  document.body.appendChild(loading);
              "#,
                  Cancellable::NONE,
                  |_| info!("Injected loading element successfully"),
              );
            })?;

            let saml_request = portal_prelogin(&portal, gp_params).await?;
            window.with_webview(move |wv| {
              let wv = wv.inner();
              load_saml_request(&wv, &saml_request);
            })?;
          }
        }
      }
    }
  }
}

fn raise_window(window: &Arc<Window>) {
  let visible = window.is_visible().unwrap_or(false);
  if !visible {
    if let Err(err) = window.raise() {
      warn!("Failed to raise window: {}", err);
    }
  }
}

pub async fn portal_prelogin(portal: &str, gp_params: &GpParams) -> anyhow::Result<String> {
  match prelogin(portal, gp_params).await? {
    Prelogin::Saml(prelogin) => Ok(prelogin.saml_request().to_string()),
    Prelogin::Standard(_) => bail!("Received non-SAML prelogin response"),
  }
}

fn send_auth_result(auth_result_tx: &mpsc::UnboundedSender<AuthResult>, auth_result: AuthResult) {
  if let Err(err) = auth_result_tx.send(auth_result) {
    warn!("Failed to send auth event: {}", err);
  }
}

fn load_saml_request(wv: &Rc<WebView>, saml_request: &str) {
  if saml_request.starts_with("http") {
    info!("Load the SAML request as URI...");
    wv.load_uri(saml_request);
  } else {
    info!("Load the SAML request as HTML...");
    wv.load_html(saml_request, None);
  }
}

fn read_auth_data_from_headers(response: &URIResponse) -> AuthResult {
  response.http_headers().map_or_else(
    || {
      info!("No headers found in response");
      Err(AuthDataError::NotFound)
    },
    |mut headers| match headers.get("saml-auth-status") {
      Some(status) if status == "1" => {
        let username = headers.get("saml-username").map(GString::into);
        let prelogin_cookie = headers.get("prelogin-cookie").map(GString::into);
        let portal_userauthcookie = headers.get("portal-userauthcookie").map(GString::into);

        if SamlAuthData::check(&username, &prelogin_cookie, &portal_userauthcookie) {
          return Ok(SamlAuthData::new(
            username.unwrap(),
            prelogin_cookie,
            portal_userauthcookie,
          ));
        }

        info!("Found invalid auth data in headers");
        Err(AuthDataError::Invalid)
      }
      Some(status) => {
        info!("Found invalid SAML status: {} in headers", status);
        Err(AuthDataError::Invalid)
      }
      None => {
        info!("No saml-auth-status header found");
        Err(AuthDataError::NotFound)
      }
    },
  )
}

fn read_auth_data_from_body<F>(main_resource: &WebResource, callback: F)
where
  F: FnOnce(Result<SamlAuthData, AuthDataParseError>) + Send + 'static,
{
  main_resource.data(Cancellable::NONE, |data| match data {
    Ok(data) => {
      let html = String::from_utf8_lossy(&data);
      callback(read_auth_data_from_html(&html));
    }
    Err(err) => {
      info!("Failed to read response body: {}", err);
      callback(Err(AuthDataParseError::Invalid))
    }
  });
}

fn read_auth_data_from_html(html: &str) -> Result<SamlAuthData, AuthDataParseError> {
  if html.contains("Temporarily Unavailable") {
    info!("Found 'Temporarily Unavailable' in HTML, auth failed");
    return Err(AuthDataParseError::Invalid);
  }

  match SamlAuthData::from_html(html) {
    Ok(auth_data) => Ok(auth_data),
    Err(err) => {
      if let Some(gpcallback) = extract_gpcallback(html) {
        info!("Found gpcallback from html...");
        SamlAuthData::from_gpcallback(&gpcallback)
      } else {
        Err(err)
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

fn read_auth_data(main_resource: &WebResource, auth_result_tx: mpsc::UnboundedSender<AuthResult>) {
  let Some(response) = main_resource.response() else {
    info!("No response found in main resource");
    send_auth_result(&auth_result_tx, Err(AuthDataError::Invalid));
    return;
  };

  info!("Trying to read auth data from response headers...");

  match read_auth_data_from_headers(&response) {
    Ok(auth_data) => {
      info!("Got auth data from headers");
      send_auth_result(&auth_result_tx, Ok(auth_data));
    }
    Err(AuthDataError::Invalid) => {
      info!("Found invalid auth data in headers, trying to read from body...");
      read_auth_data_from_body(main_resource, move |auth_result| {
        // Since we have already found invalid auth data in headers, which means this could be the `/SAML20/SP/ACS` endpoint
        // any error result from body should be considered as invalid, and trigger a retry
        let auth_result = auth_result.map_err(|err| {
          info!("Failed to read auth data from body: {}", err);
          AuthDataError::Invalid
        });
        send_auth_result(&auth_result_tx, auth_result);
      });
    }
    Err(AuthDataError::NotFound) => {
      info!("No auth data found in headers, trying to read from body...");

      let is_acs_endpoint = main_resource.uri().map_or(false, |uri| uri.contains("/SAML20/SP/ACS"));

      read_auth_data_from_body(main_resource, move |auth_result| {
        // If the endpoint is `/SAML20/SP/ACS` and no auth data found in body, it should be considered as invalid
        let auth_result = auth_result.map_err(|err| {
          info!("Failed to read auth data from body: {}", err);

          if !is_acs_endpoint && matches!(err, AuthDataParseError::NotFound) {
            AuthDataError::NotFound
          } else {
            AuthDataError::Invalid
          }
        });

        send_auth_result(&auth_result_tx, auth_result)
      });
    }
    Err(AuthDataError::TlsError) => {
      // NOTE: This is unreachable
      info!("TLS error found in headers, trying to read from body...");
      send_auth_result(&auth_result_tx, Err(AuthDataError::TlsError));
    }
  }
}

pub(crate) async fn clear_webview_cookies(window: &Window) -> anyhow::Result<()> {
  let (tx, rx) = oneshot::channel::<Result<(), String>>();

  window.with_webview(|wv| {
    let send_result = move |result: Result<(), String>| {
      if let Err(err) = tx.send(result) {
        info!("Failed to send result: {:?}", err);
      }
    };

    let wv = wv.inner();
    let context = match wv.context() {
      Some(context) => context,
      None => {
        send_result(Err("No webview context found".into()));
        return;
      }
    };
    let data_manager = match context.website_data_manager() {
      Some(manager) => manager,
      None => {
        send_result(Err("No data manager found".into()));
        return;
      }
    };

    let now = Instant::now();
    data_manager.clear(
      WebsiteDataTypes::COOKIES,
      TimeSpan(0),
      Cancellable::NONE,
      move |result| match result {
        Err(err) => {
          send_result(Err(err.to_string()));
        }
        Ok(_) => {
          info!("Cookies cleared in {} ms", now.elapsed().as_millis());
          send_result(Ok(()));
        }
      },
    );
  })?;

  rx.await?.map_err(|err| anyhow::anyhow!(err))
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
