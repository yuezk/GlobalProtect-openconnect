use anyhow::Context;
use block2::RcBlock;
use objc2::{DefinedClass, MainThreadOnly, rc::Weak};
use objc2_app_kit::{NSAlert, NSAlertSecondButtonReturn, NSAlertStyle};
use objc2_foundation::{
  NSError, NSString, NSURL, NSURLErrorDomain, NSURLErrorFailingURLErrorKey, NSURLErrorFailingURLPeerTrustErrorKey,
  NSUnderlyingErrorKey,
};
use objc2_security::{SecTrust, errSecHostNameMismatch};
use objc2_web_kit::WKWebView;

use super::super::{
  auth_messenger::{AuthError, AuthEvent},
  tls::{TlsFailure, TlsFailureReason},
};

#[derive(Clone)]
struct CertificateException {
  host: String,
  certificate: Vec<u8>,
}

#[derive(Clone)]
struct PendingCertificate {
  generation: u64,
  failure: TlsFailure,
  exception: CertificateException,
}

#[derive(Default)]
pub(super) struct TlsState {
  pending: Option<PendingCertificate>,
  exceptions: Vec<CertificateException>,
}

impl TlsState {
  fn approve(&mut self, pending: &PendingCertificate, generation: u64) -> bool {
    if pending.generation != generation
      || self.pending.as_ref().is_none_or(|current| {
        current.generation != pending.generation
          || current.exception.certificate != pending.exception.certificate
          || current.exception.host != pending.exception.host
      })
    {
      return false;
    }
    self.exceptions.push(pending.exception.clone());
    self.pending = None;
    true
  }

  pub fn record_failure(&mut self, webview: &WKWebView, error: &NSError, generation: u64) -> Option<AuthError> {
    if !error.domain().isEqualToString(unsafe { NSURLErrorDomain }) {
      return None;
    }
    let reason = match error.code() {
      -1200 => TlsFailureReason::Handshake,
      -1201 => TlsFailureReason::Expired,
      -1202 => TlsFailureReason::InvalidCertificate,
      -1203 => TlsFailureReason::Untrusted,
      -1204 => TlsFailureReason::NotYetValid,
      _ => return None,
    };
    let reason = if has_hostname_mismatch(error, 0) {
      TlsFailureReason::HostnameMismatch
    } else {
      reason
    };
    let url = error
      .userInfo()
      .objectForKey(unsafe { NSURLErrorFailingURLErrorKey })?
      .downcast::<NSURL>()
      .ok()?;
    let host = url.host()?.to_string().to_ascii_lowercase();
    if let Some(uri) = url.absoluteString() {
      super::address_bar::show_address(webview, &uri.to_string());
    }
    let trust = error
      .userInfo()
      .objectForKey(unsafe { NSURLErrorFailingURLPeerTrustErrorKey });
    let certificate = trust.and_then(|trust| {
      // Foundation documents this entry as a SecTrustRef.
      let trust = unsafe { &*(objc2::rc::Retained::as_ptr(&trust).cast::<SecTrust>()) };
      certificate_data(trust)
    });
    let failure = TlsFailure {
      host: host.clone(),
      reason,
      can_continue: reason.is_certificate_error() && certificate.is_some(),
    };
    self.pending = certificate
      .filter(|_| failure.can_continue)
      .map(|certificate| PendingCertificate {
        generation,
        failure: failure.clone(),
        exception: CertificateException { host, certificate },
      });
    Some(AuthError::Tls(failure))
  }

  pub fn allows(&self, host: &str, trust: &SecTrust) -> bool {
    if self.exceptions.is_empty() {
      return false;
    }
    let Some(certificate) = certificate_data(trust) else {
      return false;
    };
    self
      .exceptions
      .iter()
      .any(|exception| exception.host.eq_ignore_ascii_case(host) && exception.certificate == certificate)
  }
}

fn has_hostname_mismatch(error: &NSError, depth: usize) -> bool {
  if depth > 4 {
    return false;
  }
  if error.code() == errSecHostNameMismatch as isize {
    return true;
  }
  error
    .userInfo()
    .objectForKey(unsafe { NSUnderlyingErrorKey })
    .and_then(|value| value.downcast::<NSError>().ok())
    .is_some_and(|error| has_hostname_mismatch(&error, depth + 1))
}

fn certificate_data(trust: &SecTrust) -> Option<Vec<u8>> {
  // Retain the leaf immediately. No trust evaluation or mutation runs here.
  #[allow(deprecated)]
  let certificate = unsafe { trust.certificate_at_index(0) }?;
  Some(unsafe { certificate.data() }.to_vec())
}

pub(super) fn confirm(webview: &WKWebView) -> anyhow::Result<()> {
  let delegate = super::navigation_delegate(webview).context("Auth navigation delegate is unavailable")?;
  let pending = delegate
    .ivars()
    .tls
    .borrow()
    .pending
    .clone()
    .context("No certificate error is awaiting confirmation")?;
  if pending.generation != delegate.ivars().messenger.generation() {
    return Ok(());
  }
  let window = webview.window().context("Auth window is unavailable")?;
  if window.attachedSheet().is_some() {
    return Ok(());
  }
  let alert = NSAlert::new(delegate.mtm());
  alert.setAlertStyle(NSAlertStyle::Warning);
  alert.setMessageText(&NSString::from_str(&pending.failure.confirmation_title()));
  alert.setInformativeText(&NSString::from_str(&pending.failure.confirmation_message()));
  alert.addButtonWithTitle(&NSString::from_str("Cancel"));
  alert.addButtonWithTitle(&NSString::from_str("Continue Anyway"));
  let weak = Weak::from_retained(&delegate);
  let handler = RcBlock::new(move |response: isize| {
    let Some(delegate) = weak.load() else {
      return;
    };
    if response != NSAlertSecondButtonReturn || pending.generation != delegate.ivars().messenger.generation() {
      return;
    }
    let mut state = delegate.ivars().tls.borrow_mut();
    if !state.approve(&pending, delegate.ivars().messenger.generation()) {
      return;
    }
    drop(state);
    delegate
      .ivars()
      .messenger
      .send_auth_event(AuthEvent::TlsAccepted(pending.generation));
  });
  alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn pending() -> PendingCertificate {
    PendingCertificate {
      generation: 1,
      failure: TlsFailure {
        host: "login.example".into(),
        reason: TlsFailureReason::Expired,
        can_continue: true,
      },
      exception: CertificateException {
        host: "login.example".into(),
        certificate: vec![1, 2, 3],
      },
    }
  }

  #[test]
  fn certificate_approval_rejects_stale_attempts_and_replacement_certificates() {
    let original = pending();
    let mut state = TlsState {
      pending: Some(original.clone()),
      ..TlsState::default()
    };
    assert!(!state.approve(&original, 2));
    let mut replacement = original.clone();
    replacement.exception.certificate = vec![4, 5, 6];
    state.pending = Some(replacement);
    assert!(!state.approve(&original, 1));
    assert!(state.exceptions.is_empty());
  }

  #[test]
  fn certificate_approval_is_specific_to_the_host_and_consumed_once() {
    let original = pending();
    let mut state = TlsState {
      pending: Some(original.clone()),
      ..TlsState::default()
    };
    let mut other_host = original.clone();
    other_host.exception.host = "other.example".into();
    assert!(!state.approve(&other_host, 1));
    assert!(state.approve(&original, 1));
    assert_eq!(state.exceptions.len(), 1);
    assert!(!state.approve(&original, 1));
  }
}
