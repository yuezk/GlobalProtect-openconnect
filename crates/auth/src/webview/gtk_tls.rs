use std::{
  cell::{Cell, RefCell},
  rc::Rc,
  sync::Arc,
};

use anyhow::Context;
use gtk::prelude::*;
use webkit2gtk::{
  WebContextExt, WebView, WebViewExt,
  gio::{TlsCertificate, TlsCertificateFlags},
};

use super::super::{
  auth_messenger::{AuthEvent, AuthMessenger},
  tls::{TlsFailure, TlsFailureReason},
};

const TLS_KEY: &str = "gpauth-tls";

#[derive(Clone)]
struct PendingCertificate {
  generation: u64,
  certificate: TlsCertificate,
  failure: TlsFailure,
}

struct TlsState {
  pending: RefCell<Option<PendingCertificate>>,
  confirmation_open: Cell<bool>,
  messenger: Arc<AuthMessenger>,
}

pub(super) fn install(webview: &WebView, messenger: Arc<AuthMessenger>) {
  unsafe {
    webview.set_data(
      TLS_KEY,
      Rc::new(TlsState {
        pending: RefCell::new(None),
        confirmation_open: Cell::new(false),
        messenger,
      }),
    );
  }
}

fn state(webview: &WebView) -> anyhow::Result<Rc<TlsState>> {
  let data = unsafe { webview.data::<Rc<TlsState>>(TLS_KEY) }.context("TLS controls are unavailable")?;
  Ok(unsafe { data.as_ref() }.clone())
}

pub(super) fn record_failure(
  webview: &WebView,
  uri: &str,
  certificate: &TlsCertificate,
  flags: TlsCertificateFlags,
  generation: u64,
) -> TlsFailure {
  let reason = if flags.contains(TlsCertificateFlags::EXPIRED) {
    TlsFailureReason::Expired
  } else if flags.contains(TlsCertificateFlags::NOT_ACTIVATED) {
    TlsFailureReason::NotYetValid
  } else if flags.contains(TlsCertificateFlags::BAD_IDENTITY) {
    TlsFailureReason::HostnameMismatch
  } else if flags.contains(TlsCertificateFlags::UNKNOWN_CA) {
    TlsFailureReason::Untrusted
  } else {
    TlsFailureReason::InvalidCertificate
  };
  let host = url::Url::parse(uri)
    .ok()
    .filter(|uri| uri.scheme() == "https")
    .and_then(|uri| uri.host_str().map(str::to_owned));
  let failure = TlsFailure {
    host: host.clone().unwrap_or_default(),
    reason,
    can_continue: reason.is_certificate_error() && host.is_some(),
  };
  if let Ok(state) = state(webview) {
    *state.pending.borrow_mut() = host.map(|_| PendingCertificate {
      generation,
      certificate: certificate.clone(),
      failure: failure.clone(),
    });
  }
  failure
}

pub(super) fn confirm(webview: &WebView) -> anyhow::Result<()> {
  let state = state(webview)?;
  let pending = state
    .pending
    .borrow()
    .clone()
    .context("No certificate error is awaiting confirmation")?;
  if pending.generation != state.messenger.generation() || state.confirmation_open.get() {
    return Ok(());
  }
  let parent = webview
    .toplevel()
    .and_then(|widget| widget.downcast::<gtk::Window>().ok())
    .context("Auth window is unavailable")?;
  let dialog = gtk::Dialog::builder()
    .transient_for(&parent)
    .modal(true)
    .destroy_with_parent(true)
    .title("Certificate Warning")
    .use_header_bar(0)
    .resizable(false)
    .build();
  let content = dialog.content_area();
  content.set_spacing(12);
  content.set_margin_top(18);
  content.set_margin_bottom(18);
  content.set_margin_start(18);
  content.set_margin_end(18);
  let title = gtk::Label::new(None);
  title.set_markup(&format!(
    "<b>{}</b>",
    gtk::glib::markup_escape_text(&pending.failure.confirmation_title())
  ));
  let message = gtk::Label::new(Some(&pending.failure.confirmation_message()));
  for label in [&title, &message] {
    label.set_xalign(0.0);
    label.set_line_wrap(true);
    label.set_line_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_max_width_chars(48);
    content.pack_start(label, false, false, 0);
  }
  let actions = gtk::ButtonBox::new(gtk::Orientation::Horizontal);
  actions.set_layout(gtk::ButtonBoxStyle::End);
  actions.set_spacing(8);
  actions.set_margin_top(6);
  content.pack_start(&actions, false, false, 0);
  for (label, response) in [
    ("Cancel", gtk::ResponseType::Cancel),
    ("Continue Anyway", gtk::ResponseType::Accept),
  ] {
    let button = super::action_button(label)?;
    button.set_can_default(true);
    let weak_dialog = dialog.downgrade();
    button.connect_clicked(move |_| {
      if let Some(dialog) = weak_dialog.upgrade() {
        dialog.response(response);
      }
    });
    actions.pack_start(&button, false, false, 0);
    actions.set_child_non_homogeneous(&button, true);
    if response == gtk::ResponseType::Cancel {
      dialog.set_default(Some(&button));
    }
  }
  state.confirmation_open.set(true);
  let weak = webview.downgrade();
  dialog.connect_response(move |dialog, response| {
    state.confirmation_open.set(false);
    if response == gtk::ResponseType::Accept && pending.generation == state.messenger.generation() {
      if let Some(webview) = weak.upgrade() {
        let current = state.pending.borrow().as_ref().is_some_and(|current| {
          current.generation == pending.generation
            && current.certificate == pending.certificate
            && current.failure.host == pending.failure.host
        });
        if current {
          if let Some(context) = webview.context() {
            // This single-window auth process discards its WebContext on exit.
            context.allow_tls_certificate_for_host(&pending.certificate, &pending.failure.host);
            state.pending.borrow_mut().take();
            state
              .messenger
              .send_auth_event(AuthEvent::TlsAccepted(pending.generation));
          }
        }
      }
    }
    dialog.close();
  });
  dialog.show_all();
  Ok(())
}
