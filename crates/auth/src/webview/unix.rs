use std::sync::Arc;

use gpapi::utils::redact::redact_uri;
use log::warn;
use webkit2gtk::{
  gio::Cancellable, glib::GString, LoadEvent, URIResponseExt, WebResource, WebResourceExt, WebView, WebViewExt,
};
use wry::WebViewExtUnix;

use crate::webview::auth_messenger::AuthError;

pub struct AuthResponse {
  web_resource: WebResource,
}

impl AuthResponse {
  pub fn url(&self) -> Option<String> {
    self.web_resource.uri().map(GString::into)
  }

  pub fn get_header(&self, key: &str) -> Option<String> {
    self
      .web_resource
      .response()
      .and_then(|response| response.http_headers())
      .and_then(|headers| headers.one(key))
      .map(GString::into)
  }

  pub fn get_body<F>(&self, cb: F)
  where
    F: FnOnce(anyhow::Result<Vec<u8>>) + 'static,
  {
    let cancellable = Cancellable::NONE;
    self.web_resource.data(cancellable, move |data| {
      cb(data.map_err(|e| anyhow::anyhow!(e)));
    });
  }
}

pub fn connect_webview_response<F>(wv: &wry::WebView, cb: F)
where
  F: Fn(anyhow::Result<AuthResponse, AuthError>) + 'static,
{
  let wv = wv.webview();
  let cb = Arc::new(cb);

  let cb_clone = Arc::clone(&cb);
  wv.connect_load_changed(move |wv, event| {
    if event == LoadEvent::Started {
      // TODO;
      // auth_messenger_clone.cancel_raise_window();
      return;
    }

    if event != LoadEvent::Finished {
      return;
    }

    let Some(web_resource) = wv.main_resource() else {
      return;
    };

    let uri = web_resource.uri().unwrap_or("".into());
    if uri.is_empty() {
      warn!("Loaded an empty URI");
      cb_clone(Err(AuthError::Invalid));
      return;
    }

    let response = AuthResponse { web_resource };

    cb_clone(Ok(response));
  });

  wv.connect_load_failed_with_tls_errors(move |_wv, uri, cert, err| {
    let redacted_uri = redact_uri(uri);
    warn!(
      "Failed to load uri: {} with error: {}, cert: {}",
      redacted_uri, err, cert
    );

    cb(Err(AuthError::TlsError));
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
}
