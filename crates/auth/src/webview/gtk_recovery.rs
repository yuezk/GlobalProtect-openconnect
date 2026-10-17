use std::sync::Arc;

use anyhow::Context;
use gtk::prelude::*;
use webkit2gtk::WebView;

use super::super::{
  auth_messenger::{AuthEvent, AuthMessenger},
  recovery::RecoveryStatus,
};

const CONTROLS_KEY: &str = "gpauth-recovery-controls";

struct RecoveryRow {
  row: gtk::Box,
  spinner: gtk::Spinner,
  label: gtk::Label,
  retry: gtk::Button,
  proceed: gtk::Button,
  cancel: gtk::Button,
  detail: gtk::Label,
}

pub(super) fn install(webview: &WebView, header: &gtk::Box, messenger: Arc<AuthMessenger>) -> anyhow::Result<()> {
  let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
  // Align status text with the address text's 6px inset and native 1px border.
  row.set_margin_start(7);
  let spinner = gtk::Spinner::new();
  spinner.set_valign(gtk::Align::Center);
  let label = gtk::Label::new(None);
  label.set_xalign(0.0);
  label.set_ellipsize(gtk::pango::EllipsizeMode::End);
  let detail = gtk::Label::new(None);
  detail.set_xalign(0.0);
  detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
  detail.style_context().add_class("dim-label");
  detail.set_no_show_all(true);
  let copy = gtk::Box::new(gtk::Orientation::Vertical, 2);
  copy.set_valign(gtk::Align::Center);
  copy.pack_start(&label, false, false, 0);
  copy.pack_start(&detail, false, false, 0);
  copy.show();
  let retry = super::action_button("Try Again")?;
  retry.set_valign(gtk::Align::Center);
  let proceed = super::action_button("Continue Anyway…")?;
  proceed.set_valign(gtk::Align::Center);
  proceed.set_no_show_all(true);
  let cancel = super::action_button("Cancel")?;
  cancel.set_valign(gtk::Align::Center);
  // Showing or raising the window must not override recovery-state visibility.
  row.set_no_show_all(true);
  spinner.set_no_show_all(true);
  retry.set_no_show_all(true);
  label.show();
  cancel.set_no_show_all(true);
  let retry_messenger = Arc::clone(&messenger);
  retry.connect_clicked(move |_| retry_messenger.send_auth_event(AuthEvent::Retry));
  let proceed_messenger = Arc::clone(&messenger);
  proceed.connect_clicked(move |_| proceed_messenger.send_auth_event(AuthEvent::RequestTlsException));
  cancel.connect_clicked(move |_| messenger.send_auth_event(AuthEvent::Close));
  row.pack_start(&spinner, false, false, 0);
  row.pack_start(&copy, true, true, 0);
  row.pack_start(&retry, false, false, 0);
  row.pack_start(&proceed, false, false, 0);
  row.pack_start(&cancel, false, false, 0);
  header.pack_start(&row, false, false, 0);
  // The WebView owns the controls for exactly its native lifetime, on the GTK thread.
  unsafe {
    webview.set_data(
      CONTROLS_KEY,
      RecoveryRow {
        row,
        spinner,
        label,
        retry,
        proceed,
        cancel,
        detail,
      },
    );
  }
  Ok(())
}

pub(super) fn update(webview: &WebView, status: RecoveryStatus) -> anyhow::Result<()> {
  let controls = unsafe { webview.data::<RecoveryRow>(CONTROLS_KEY) }.context("Recovery controls are unavailable")?;
  let controls = unsafe { controls.as_ref() };
  controls.label.set_text(status.message());
  controls.detail.set_text(status.detail());
  controls.detail.set_visible(!status.detail().is_empty());
  controls.retry.set_visible(status.can_retry());
  controls.proceed.set_visible(status.can_continue());
  controls.cancel.set_visible(status.blocks_page());
  controls.spinner.set_visible(status.is_busy());
  if status.is_busy() {
    controls.spinner.start();
  } else {
    controls.spinner.stop();
  }
  controls.row.set_visible(status != RecoveryStatus::Hidden);
  webview.set_sensitive(!status.blocks_page());
  Ok(())
}
