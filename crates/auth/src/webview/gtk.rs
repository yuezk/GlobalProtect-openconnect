use std::{cell::Cell, rc::Rc, sync::Arc};

use anyhow::{Context, bail};
use gpapi::utils::redact::redact_uri;
use gtk::prelude::*;
use log::{info, warn};
use tauri::webview::PlatformWebview;
use webkit2gtk::{
  LoadEvent, SettingsExt, TLSErrorsPolicy, URIResponseExt, WebResource, WebResourceExt, WebViewExt,
  WebsiteDataManagerExt, gio::Cancellable, glib::GString,
};

use super::{
  auth_messenger::{AuthError, AuthEvent, AuthMessenger},
  recovery::RecoveryStatus,
  webview_auth::{GetHeader, PlatformWebviewExt},
};

#[path = "gtk_recovery.rs"]
mod recovery_row;
#[path = "gtk_tls.rs"]
mod tls;

fn action_button(label: &str) -> anyhow::Result<gtk::Button> {
  let button = gtk::Button::with_label(label);
  let spacing = gtk::CssProvider::new();
  spacing.load_from_data(b"button { padding: 4px 12px; }")?;
  button
    .style_context()
    .add_provider(&spacing, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
  Ok(button)
}

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
  fn install_auth_controls(&self, window: &tauri::WebviewWindow, messenger: Arc<AuthMessenger>) -> anyhow::Result<()> {
    use super::address_bar::display_address;

    let entry = gtk::Entry::new();
    entry.set_editable(false);
    entry.set_can_focus(true);
    entry.set_direction(gtk::TextDirection::Ltr);
    entry.set_text(&display_address(self.inner().uri().as_deref()));
    let header = gtk::Box::new(gtk::Orientation::Vertical, 4);
    header.style_context().add_class(gtk::STYLE_CLASS_TOOLBAR);
    header.style_context().add_class(gtk::STYLE_CLASS_PRIMARY_TOOLBAR);
    header.style_context().add_class("gpauth-header");
    // One native header owns the outer padding and the gap between its rows.
    // Keep theme colors, borders, focus styling, and natural button sizes.
    let spacing = gtk::CssProvider::new();
    spacing.load_from_data(b".gpauth-header { padding: 6px; } entry { min-height: 22px; padding: 2px 6px; }")?;
    entry
      .style_context()
      .add_provider(&spacing, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    header
      .style_context()
      .add_provider(&spacing, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    header.pack_start(&entry, false, false, 0);
    header.show_all();
    recovery_row::install(&self.inner(), &header, Arc::clone(&messenger))?;
    let container = window.default_vbox()?;
    container.pack_start(&header, false, false, 0);
    container.reorder_child(&header, 0);
    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    container.pack_start(&separator, false, false, 0);
    container.reorder_child(&separator, 1);
    separator.show();

    // Observe the top-level URI, including redirects and history API changes.
    let address = entry.clone();
    self.inner().connect_uri_notify(move |webview| {
      address.set_text(&display_address(webview.uri().as_deref()));
      address.set_position(0);
    });
    tls::install(&self.inner(), Arc::clone(&messenger));
    install_navigation_observers(&self.inner(), messenger, entry);
    Ok(())
  }

  fn set_recovery_status(&self, status: RecoveryStatus) -> anyhow::Result<()> {
    recovery_row::update(&self.inner(), status)
  }

  fn stop_loading(&self) {
    self.inner().stop_loading();
  }

  fn confirm_tls_exception(&self) -> anyhow::Result<()> {
    tls::confirm(&self.inner())
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
    prepare_navigation(&self.inner())?;
    self.inner().load_uri(url);
    Ok(())
  }

  fn load_html(&self, html: &str) -> anyhow::Result<()> {
    prepare_navigation(&self.inner())?;
    self.inner().load_html(html, None);
    Ok(())
  }
}

const NAVIGATION_KEY: &str = "gpauth-navigation";

#[derive(Default)]
struct NavigationAttempts {
  pending: Cell<Option<u64>>,
  current: Cell<Option<u64>>,
}

impl NavigationAttempts {
  fn started(&self, generation: u64) {
    self.current.set(Some(self.pending.take().unwrap_or(generation)));
  }
}

struct NavigationObserver {
  attempts: NavigationAttempts,
  messenger: Arc<AuthMessenger>,
}

fn prepare_navigation(webview: &webkit2gtk::WebView) -> anyhow::Result<()> {
  let observer =
    unsafe { webview.data::<Rc<NavigationObserver>>(NAVIGATION_KEY) }.context("Navigation observer is unavailable")?;
  let observer = unsafe { observer.as_ref() };
  observer.attempts.pending.set(Some(observer.messenger.generation()));
  Ok(())
}

fn install_navigation_observers(webview: &webkit2gtk::WebView, messenger: Arc<AuthMessenger>, address: gtk::Entry) {
  let observer = Rc::new(NavigationObserver {
    attempts: NavigationAttempts::default(),
    messenger,
  });
  unsafe {
    webview.set_data(NAVIGATION_KEY, Rc::clone(&observer));
  }
  let loading = Rc::clone(&observer);
  webview.connect_load_changed(move |webview, event| {
    match event {
      LoadEvent::Started => {
        loading.attempts.started(loading.messenger.generation());
        loading.messenger.cancel_raise_window();
      }
      LoadEvent::Committed => {
        if let Some(generation) = loading.attempts.current.get() {
          info!(
            "Started loading page: {}",
            redact_uri(webview.uri().as_deref().unwrap_or_default())
          );
          read_navigation_html(webview, Arc::clone(&loading.messenger), generation);
        }
      }
      LoadEvent::Finished => {
        // WebKitGTK finishes a cancelled load before starting its replacement.
        // Retain that load's generation instead of relabeling it at callback time.
        let Some(generation) = loading.attempts.current.take() else {
          return;
        };
        info!(
          "Finished loading page: {}",
          redact_uri(webview.uri().as_deref().unwrap_or_default())
        );
        read_navigation_html(webview, Arc::clone(&loading.messenger), generation);
        if let Some(resource) = webview.main_resource() {
          let uri = resource.uri().unwrap_or_default();
          if uri.is_empty() {
            loading.messenger.send_auth_event(AuthEvent::Error(
              generation,
              AuthError::invalid_from_body(anyhow::anyhow!("Empty URI")),
            ));
          } else {
            loading.messenger.read_from_response(&resource, generation);
          }
        }
        loading.messenger.send_auth_event(AuthEvent::PageLoaded(generation));
      }
      _ => {}
    }
  });

  let tls = Rc::clone(&observer);
  let tls_address = address.clone();
  webview.connect_load_failed_with_tls_errors(move |webview, uri, cert, err| {
    warn!(
      "Failed to load uri: {} with error: {}, cert: {}",
      redact_uri(uri),
      err,
      cert
    );
    if let Some(generation) = tls.attempts.current.take() {
      if generation != tls.messenger.generation() {
        return true;
      }
      show_failed_address(&tls_address, uri);
      let failure = tls::record_failure(webview, uri, cert, err, generation);
      tls
        .messenger
        .send_auth_event(AuthEvent::Error(generation, AuthError::Tls(failure)));
    }
    true
  });

  webview.connect_load_failed(move |_webview, _event, uri, err| {
    if let Some(generation) = observer.attempts.current.take() {
      if generation != observer.messenger.generation() {
        return true;
      }
      if !uri.starts_with("globalprotectcallback:") && !err.matches(webkit2gtk::NetworkError::Cancelled) {
        warn!("Failed to load uri: {} with error: {}", redact_uri(uri), err);
        show_failed_address(&address, uri);
        let error = if err.kind::<webkit2gtk::gio::TlsError>().is_some() {
          AuthError::Tls(super::tls::TlsFailure {
            host: url::Url::parse(uri)
              .ok()
              .and_then(|uri| uri.host_str().map(str::to_owned))
              .unwrap_or_default(),
            reason: super::tls::TlsFailureReason::Handshake,
            can_continue: false,
          })
        } else {
          AuthError::LoadFailed(anyhow::anyhow!("{}", err))
        };
        observer.messenger.send_auth_event(AuthEvent::Error(generation, error));
      }
    }
    true
  });
}

// A failed provisional load may leave WebView.uri unset.
fn show_failed_address(entry: &gtk::Entry, uri: &str) {
  entry.set_text(&super::address_bar::display_address(Some(uri)));
  entry.set_position(0);
}

fn read_navigation_html(webview: &webkit2gtk::WebView, messenger: Arc<AuthMessenger>, generation: u64) {
  webview.evaluate_javascript(
    "document.documentElement.outerHTML",
    None,
    None,
    Cancellable::NONE,
    move |result| match result {
      Ok(value) => messenger.read_from_html(&value.to_string(), generation),
      Err(err) => warn!("Failed to get html: {}", err),
    },
  );
}

#[cfg(test)]
mod tests {
  use super::NavigationAttempts;

  #[test]
  fn cancelled_load_keeps_its_generation_until_replacement_starts() {
    let attempts = NavigationAttempts::default();
    attempts.started(0);
    attempts.pending.set(Some(2));
    // The previous load's cancellation/finish arrives after retry generation advances.
    assert_eq!(attempts.current.take(), Some(0));
    attempts.started(2);
    assert_eq!(attempts.current.take(), Some(2));
  }

  #[test]
  fn requested_generation_survives_attempt_changes_before_native_start() {
    let attempts = NavigationAttempts::default();
    attempts.pending.set(Some(2));
    attempts.started(3);
    assert_eq!(attempts.current.take(), Some(2));
    // User navigation after that load starts in the current attempt.
    attempts.started(3);
    assert_eq!(attempts.current.take(), Some(3));
  }
}
