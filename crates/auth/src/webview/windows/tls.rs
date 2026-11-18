use super::super::tls::{TlsFailure, TlsFailureReason};
use super::*;
use std::collections::HashSet;
use webview2_com::{ClearServerCertificateErrorActionsCompletedHandler, ServerCertificateErrorDetectedEventHandler};
use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW};

#[derive(Default)]
pub(super) struct Policy {
  pub ignore_errors: bool,
  approved: HashSet<(String, String)>,
  pending: Option<Failure>,
}
#[derive(Clone)]
struct Failure {
  identity: (String, String),
  failure: TlsFailure,
  generation: u64,
}

fn reason(error: COREWEBVIEW2_WEB_ERROR_STATUS) -> TlsFailureReason {
  match error {
    COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_COMMON_NAME_IS_INCORRECT => TlsFailureReason::HostnameMismatch,
    COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_EXPIRED => TlsFailureReason::Expired,
    _ => TlsFailureReason::InvalidCertificate,
  }
}

pub(super) fn install(state: &Rc<State>) -> windows::core::Result<()> {
  let weak = Rc::downgrade(state);
  let handler = ServerCertificateErrorDetectedEventHandler::create(Box::new(move |_, args| {
    let (Some(state), Some(args)) = (weak.upgrade(), args) else {
      return Ok(());
    };
    unsafe {
      let uri = text(|p| args.RequestUri(p))?;
      let host = url::Url::parse(&uri)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default();
      let pem = text(|p| args.ServerCertificate()?.ToPemEncoding(p))?;
      let mut error = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
      args.ErrorStatus(&mut error)?;
      let identity = (host.clone(), pem);
      let mut policy = state.tls.borrow_mut();
      if policy.ignore_errors || policy.approved.contains(&identity) {
        args.SetAction(COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW)?;
      } else {
        args.SetAction(COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_CANCEL)?;
        let generation = state.document.borrow().1;
        let failure = TlsFailure {
          host,
          reason: reason(error),
          can_continue: error != COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_REVOKED,
        };
        state
          .messenger
          .send_auth_event(AuthEvent::Error(generation, AuthError::Tls(failure.clone())));
        policy.pending = Some(Failure {
          identity,
          failure,
          generation,
        });
      }
    }
    Ok(())
  }));
  let mut token = 0;
  unsafe {
    state
      .core
      .cast::<ICoreWebView2_14>()?
      .add_ServerCertificateErrorDetected(&handler, &mut token)
  }
}

pub(super) fn confirm(state: &State) -> anyhow::Result<()> {
  let pending = state
    .tls
    .borrow()
    .pending
    .clone()
    .context("No certificate error to confirm")?;
  if !pending.failure.can_continue || pending.generation != state.messenger.generation() {
    bail!("This certificate exception is no longer available");
  }
  let answer = unsafe {
    MessageBoxW(
      Some(state.controls.parent),
      &HSTRING::from(pending.failure.confirmation_message()),
      &HSTRING::from(pending.failure.confirmation_title()),
      MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
    )
  };
  if answer == IDYES {
    let identity = pending.identity.clone();
    let generation = pending.generation;
    state.tls.borrow_mut().approved.insert(identity);
    state.messenger.send_auth_event(AuthEvent::TlsAccepted(generation));
  }
  Ok(())
}

// WebView2 caches approvals in the session. Do not leave them for another sign-in window.
pub(super) fn clear_exceptions(core: &ICoreWebView2) {
  if let Ok(core) = core.cast::<ICoreWebView2_14>() {
    let _ = unsafe {
      core.ClearServerCertificateErrorActions(&ClearServerCertificateErrorActionsCompletedHandler::create(Box::new(
        |_| Ok(()),
      )))
    };
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn maps_certificate_failure_reasons() {
    assert_eq!(
      reason(COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_COMMON_NAME_IS_INCORRECT),
      TlsFailureReason::HostnameMismatch
    );
    assert_eq!(
      reason(COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_EXPIRED),
      TlsFailureReason::Expired
    );
  }
}
