use std::sync::Arc;

use anyhow::bail;
use gpapi::utils::redact::redact_uri;
use log::{info, warn};
use webkit2gtk::{
  gio::Cancellable,
  glib::{GString, TimeSpan},
  LoadEvent, TLSErrorsPolicy, URIResponseExt, WebResource, WebResourceExt, WebView, WebViewExt, WebsiteDataManagerExt,
  WebsiteDataManagerExtManual, WebsiteDataTypes,
};

use crate::webview_auth::{
  auth_messenger::AuthError,
  auth_response::read_auth_data,
  auth_settings::{AuthRequest, AuthSettings},
};

use super::auth_response::AuthResponse;

impl AuthResponse for WebResource {
  fn get_header(&self, key: &str) -> Option<String> {
    self
      .response()
      .and_then(|response| response.http_headers())
      .and_then(|headers| headers.one(key))
      .map(GString::into)
  }

  fn get_body<F>(&self, cb: F)
  where
    F: FnOnce(anyhow::Result<Vec<u8>>) + 'static,
  {
    let cancellable = Cancellable::NONE;
    self.data(cancellable, |data| cb(data.map_err(|e| anyhow::anyhow!(e))));
  }

  fn url(&self) -> Option<String> {
    self.uri().map(GString::into)
  }
}

pub fn clear_data<F>(wv: &WebView, cb: F)
where
  F: FnOnce(anyhow::Result<()>) + Send + 'static,
{
  let Some(data_manager) = wv.website_data_manager() else {
    cb(Err(anyhow::anyhow!("Failed to get website data manager")));
    return;
  };

  data_manager.clear(
    WebsiteDataTypes::COOKIES,
    TimeSpan(0),
    Cancellable::NONE,
    move |result| {
      cb(result.map_err(|e| anyhow::anyhow!(e)));
    },
  );
}

pub fn setup_webview(wv: &WebView, auth_settings: AuthSettings) -> anyhow::Result<()> {
  let AuthSettings {
    auth_request,
    auth_messenger,
    ignore_tls_errors,
  } = auth_settings;
  let auth_messenger_clone = Arc::clone(&auth_messenger);

  let Some(data_manager) = wv.website_data_manager() else {
    bail!("Failed to get website data manager");
  };

  if ignore_tls_errors {
    data_manager.set_tls_errors_policy(TLSErrorsPolicy::Ignore);
  }

  wv.connect_load_changed(move |wv, event| {
    if event == LoadEvent::Started {
      auth_messenger_clone.cancel_raise_window();
      return;
    }

    if event != LoadEvent::Finished {
      return;
    }

    let Some(main_resource) = wv.main_resource() else {
      return;
    };

    let uri = main_resource.uri().unwrap_or("".into());
    if uri.is_empty() {
      warn!("Loaded an empty URI");
      auth_messenger_clone.send_auth_error(AuthError::Invalid);
      return;
    }

    read_auth_data(&main_resource, &auth_messenger_clone);
  });

  wv.connect_load_failed_with_tls_errors(move |_wv, uri, cert, err| {
    let redacted_uri = redact_uri(uri);
    warn!(
      "Failed to load uri: {} with error: {}, cert: {}",
      redacted_uri, err, cert
    );

    auth_messenger.send_auth_error(AuthError::TlsError);
    true
  });

  wv.connect_load_failed(move |_wv, _event, uri, err| {
    let redacted_uri = redact_uri(uri);
    if !uri.starts_with("globalprotectcallback:") {
      warn!("Failed to load uri: {} with error: {}", redacted_uri, err);
    }
    // NOTE: Don't send error here, since load_changed event will be triggered after this
    // true to stop other handlers from being invoked for the event. false to propagate the event further.
    true
  });

  load_auth_request(wv, &auth_request);

  Ok(())
}

pub fn load_auth_request(wv: &WebView, auth_request: &AuthRequest) {
  if auth_request.is_url() {
    info!("Loading auth request as URI...");
    wv.load_uri(auth_request.as_str());
  } else {
    info!("Loading auth request as HTML...");
    wv.load_html(auth_request.as_str(), None);
  }
}
