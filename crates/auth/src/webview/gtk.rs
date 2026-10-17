use std::sync::Arc;

use anyhow::bail;
use gpapi::utils::redact::redact_uri;
use gtk::prelude::*;
use log::warn;
use tauri::webview::PlatformWebview;
use webkit2gtk::{
  LoadEvent, SettingsExt, TLSErrorsPolicy, URIResponseExt, WebResource, WebResourceExt, WebViewExt,
  WebsiteDataManagerExt, gio::Cancellable, glib::GString,
};

use super::{
  auth_messenger::AuthError,
  webview_auth::{GetHeader, PlatformWebviewExt},
};

impl GetHeader for WebResource {
  fn get_header(&self, key: &str) -> Option<String> {
    self
      .response()
      .and_then(|response| response.http_headers())
      .and_then(|headers| headers.one(key))
      .map(GString::into)
  }
}

impl PlatformWebviewExt for PlatformWebview {
  fn install_address_bar(&self, window: &tauri::WebviewWindow) -> anyhow::Result<()> {
    use super::address_bar::display_address;

    let entry = gtk::Entry::new();
    entry.set_editable(false);
    entry.set_can_focus(true);
    entry.set_direction(gtk::TextDirection::Ltr);
    entry.set_text(&display_address(self.inner().uri().as_deref()));
    entry.set_margin_start(6);
    entry.set_margin_end(6);
    let item = gtk::ToolItem::new();
    ToolItemExt::set_expand(&item, true);
    item.add(&entry);
    let bar = gtk::Toolbar::new();
    bar.style_context().add_class(gtk::STYLE_CLASS_PRIMARY_TOOLBAR);
    // Keep native colors, borders, and focus styling while reducing vertical space.
    let spacing = gtk::CssProvider::new();
    spacing.load_from_data(b"toolbar { padding: 2px 0; } entry { min-height: 22px; padding: 2px 6px; }")?;
    entry
      .style_context()
      .add_provider(&spacing, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    bar
      .style_context()
      .add_provider(&spacing, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    bar.insert(&item, -1);
    let container = window.default_vbox()?;
    container.pack_start(&bar, false, false, 0);
    container.reorder_child(&bar, 0);
    bar.show_all();
    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    container.pack_start(&separator, false, false, 0);
    container.reorder_child(&separator, 1);
    separator.show();

    // Observe the top-level URI, including redirects and history API changes.
    self.inner().connect_uri_notify(move |webview| {
      entry.set_text(&display_address(webview.uri().as_deref()));
      entry.set_position(0);
    });
    Ok(())
  }

  fn ignore_tls_errors(&self) -> anyhow::Result<()> {
    if let Some(manager) = self.inner().website_data_manager() {
      manager.set_tls_errors_policy(TLSErrorsPolicy::Ignore);
      return Ok(());
    }
    bail!("Failed to get website data manager");
  }

  fn user_agent(&self) -> anyhow::Result<String> {
    let Some(settings) = WebViewExt::settings(&self.inner()) else {
      bail!("Failed to get webview settings");
    };

    settings
      .user_agent()
      .map(GString::into)
      .ok_or_else(|| anyhow::anyhow!("Failed to get webview user agent"))
  }

  fn set_user_agent(&self, user_agent: &str) -> anyhow::Result<()> {
    let Some(settings) = WebViewExt::settings(&self.inner()) else {
      bail!("Failed to get webview settings");
    };

    settings.set_user_agent(Some(user_agent));
    Ok(())
  }

  fn load_url(&self, url: &str) -> anyhow::Result<()> {
    self.inner().load_uri(url);
    Ok(())
  }

  fn load_html(&self, html: &str) -> anyhow::Result<()> {
    self.inner().load_html(html, None);
    Ok(())
  }

  fn get_html(&self, callback: Box<dyn Fn(anyhow::Result<String>) + 'static>) {
    let script = "document.documentElement.outerHTML";
    self
      .inner()
      .evaluate_javascript(script, None, None, Cancellable::NONE, move |result| match result {
        Ok(value) => callback(Ok(value.to_string())),
        Err(err) => callback(Err(anyhow::anyhow!(err))),
      });
  }
}

pub trait PlatformWebviewOnResponse {
  fn on_response(&self, callback: Box<dyn Fn(anyhow::Result<WebResource, AuthError>) + 'static>);
}

impl PlatformWebviewOnResponse for PlatformWebview {
  fn on_response(&self, callback: Box<dyn Fn(anyhow::Result<WebResource, AuthError>) + 'static>) {
    let wv = self.inner();
    let callback = Arc::new(callback);
    let callback_clone = Arc::clone(&callback);

    wv.connect_load_changed(move |wv, event| {
      if event != LoadEvent::Finished {
        return;
      }

      let Some(web_resource) = wv.main_resource() else {
        return;
      };

      let uri = web_resource.uri().unwrap_or("".into());
      if uri.is_empty() {
        callback_clone(Err(AuthError::invalid_from_body(anyhow::anyhow!("Empty URI"))));
      } else {
        callback_clone(Ok(web_resource));
      }
    });

    wv.connect_load_failed_with_tls_errors(move |_wv, uri, cert, err| {
      let redacted_uri = redact_uri(uri);
      warn!(
        "Failed to load uri: {} with error: {}, cert: {}",
        redacted_uri, err, cert
      );

      callback(Err(AuthError::TlsError));
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
}
