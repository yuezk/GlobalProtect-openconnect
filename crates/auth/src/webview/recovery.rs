use super::tls::TlsFailureReason;

/// Native UI state for recovering an embedded sign-in session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum RecoveryStatus {
  #[default]
  Hidden,
  Restarting,
  Loading,
  Failed,
  Rejected,
  Tls {
    reason: TlsFailureReason,
    can_continue: bool,
  },
  CertificateBypassed,
}

impl RecoveryStatus {
  pub fn message(self) -> &'static str {
    match self {
      Self::Hidden => "",
      Self::Restarting => "Restarting sign-in…",
      Self::Loading => "Loading sign-in…",
      Self::Failed => "Couldn’t restart sign-in.",
      Self::Rejected => "Sign-in wasn’t completed.",
      Self::Tls {
        reason: TlsFailureReason::Handshake,
        ..
      } => "Secure connection failed.",
      Self::Tls { .. } => "Cannot verify this server’s identity.",
      Self::CertificateBypassed => "Certificate verification bypassed in this window.",
    }
  }

  pub fn detail(self) -> &'static str {
    match self {
      Self::Tls { reason, .. } => reason.message(),
      _ => "",
    }
  }

  pub fn can_continue(self) -> bool {
    match self {
      Self::Tls { reason, can_continue } => can_continue && reason.is_certificate_error(),
      _ => false,
    }
  }

  pub fn blocks_page(self) -> bool {
    !matches!(self, Self::Hidden | Self::CertificateBypassed)
  }

  pub fn accepts_auth_data(self) -> bool {
    matches!(self, Self::Hidden | Self::Loading | Self::CertificateBypassed)
  }

  pub fn is_busy(self) -> bool {
    matches!(self, Self::Restarting | Self::Loading)
  }

  pub fn can_retry(self) -> bool {
    matches!(
      self,
      Self::Failed
        | Self::Rejected
        | Self::Tls {
          reason: TlsFailureReason::Handshake,
          ..
        }
    )
  }
}

#[derive(Default)]
pub(super) struct Recovery {
  status: RecoveryStatus,
  automatic_retry_used: bool,
  certificate_bypassed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RetrySource {
  FreshPrelogin,
  SuppliedRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RetryTiming {
  Immediate,
  AutomaticDelay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RetryPlan {
  pub source: RetrySource,
  pub timing: RetryTiming,
}

impl Recovery {
  pub fn status(&self) -> RecoveryStatus {
    self.status
  }

  pub fn loading(&mut self) {
    self.status = RecoveryStatus::Loading;
  }

  pub fn load_failed(&mut self) {
    self.status = RecoveryStatus::Failed;
  }

  pub fn tls_failed(&mut self, reason: TlsFailureReason, can_continue: bool) {
    self.status = RecoveryStatus::Tls { reason, can_continue };
  }

  pub fn certificate_accepted(&mut self) -> Option<RetryPlan> {
    if !self.status.can_continue() {
      return None;
    }
    self.certificate_bypassed = true;
    self.status = RecoveryStatus::Restarting;
    Some(RetryPlan {
      source: RetrySource::SuppliedRequest,
      timing: RetryTiming::Immediate,
    })
  }

  /// Temporary service failures get one automatic restart per dialog.
  /// Duplicate failures while restarting or awaiting user action are ignored.
  pub fn failure(&mut self, temporary: bool) -> Option<RetryPlan> {
    if !matches!(
      self.status,
      RecoveryStatus::Hidden | RecoveryStatus::Loading | RecoveryStatus::CertificateBypassed
    ) {
      return None;
    }
    if temporary && !self.automatic_retry_used {
      self.automatic_retry_used = true;
      self.status = RecoveryStatus::Restarting;
      return Some(RetryPlan {
        source: RetrySource::FreshPrelogin,
        timing: RetryTiming::AutomaticDelay,
      });
    }
    self.status = if temporary {
      RecoveryStatus::Failed
    } else {
      RecoveryStatus::Rejected
    };
    None
  }

  pub fn retry(&mut self) -> Option<RetryPlan> {
    if !self.status.can_retry() {
      return None;
    }
    let source = if matches!(self.status, RecoveryStatus::Tls { .. }) {
      RetrySource::SuppliedRequest
    } else {
      RetrySource::FreshPrelogin
    };
    self.status = RecoveryStatus::Restarting;
    Some(RetryPlan {
      source,
      timing: RetryTiming::Immediate,
    })
  }

  pub fn page_loaded(&mut self) {
    if self.status == RecoveryStatus::Loading {
      self.status = if self.certificate_bypassed {
        RecoveryStatus::CertificateBypassed
      } else {
        RecoveryStatus::Hidden
      };
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn retry_plans_preserve_request_source_and_timing() {
    let mut recovery = Recovery::default();
    assert_eq!(
      recovery.failure(true),
      Some(RetryPlan {
        source: RetrySource::FreshPrelogin,
        timing: RetryTiming::AutomaticDelay,
      })
    );
    recovery.load_failed();
    assert_eq!(
      recovery.retry(),
      Some(RetryPlan {
        source: RetrySource::FreshPrelogin,
        timing: RetryTiming::Immediate,
      })
    );
    recovery.tls_failed(TlsFailureReason::Expired, true);
    assert_eq!(
      recovery.certificate_accepted(),
      Some(RetryPlan {
        source: RetrySource::SuppliedRequest,
        timing: RetryTiming::Immediate,
      })
    );
    assert!(recovery.certificate_accepted().is_none());
  }

  #[test]
  fn tls_failure_blocks_credentials_and_is_not_cleared_by_page_completion() {
    let mut recovery = Recovery {
      status: RecoveryStatus::Tls {
        reason: TlsFailureReason::Expired,
        can_continue: true,
      },
      ..Recovery::default()
    };
    assert!(recovery.status.blocks_page());
    assert!(!recovery.status.accepts_auth_data());
    assert!(recovery.status.can_continue());
    recovery.page_loaded();
    assert!(matches!(recovery.status, RecoveryStatus::Tls { .. }));
    assert!(recovery.failure(true).is_none());
    assert!(matches!(recovery.status, RecoveryStatus::Tls { .. }));
    assert!(recovery.retry().is_none());
    assert!(recovery.status.can_continue());
  }

  #[test]
  fn handshake_failures_never_offer_a_certificate_exception() {
    let status = RecoveryStatus::Tls {
      reason: TlsFailureReason::Handshake,
      can_continue: true,
    };
    assert!(!status.can_continue());
    assert!(status.can_retry());
    assert!(status.blocks_page());
  }

  #[test]
  fn certificate_notice_keeps_successful_page_interactive() {
    let mut recovery = Recovery {
      certificate_bypassed: true,
      status: RecoveryStatus::Loading,
      ..Recovery::default()
    };
    recovery.page_loaded();
    assert_eq!(recovery.status, RecoveryStatus::CertificateBypassed);
    assert!(!recovery.status.blocks_page());
    assert!(recovery.status.accepts_auth_data());
    assert!(recovery.failure(false).is_none());
    assert_eq!(recovery.status, RecoveryStatus::Rejected);
  }

  #[test]
  fn temporary_failure_retries_once_even_after_manual_restarts() {
    let mut recovery = Recovery::default();
    assert!(recovery.failure(true).is_some());
    assert!(recovery.failure(true).is_none());
    recovery.status = RecoveryStatus::Loading;
    recovery.page_loaded();
    assert!(recovery.failure(true).is_none());
    assert_eq!(recovery.status, RecoveryStatus::Failed);
    assert!(recovery.retry().is_some());
    recovery.status = RecoveryStatus::Loading;
    assert!(recovery.failure(true).is_none());
    assert_eq!(recovery.status, RecoveryStatus::Failed);
  }

  #[test]
  fn rejected_auth_requires_user_action_and_deduplicates_errors() {
    let mut recovery = Recovery::default();
    assert!(recovery.failure(false).is_none());
    assert_eq!(recovery.status, RecoveryStatus::Rejected);
    assert!(recovery.failure(true).is_none());
    assert!(recovery.retry().is_some());
    assert!(recovery.retry().is_none());
  }

  #[test]
  fn page_completion_does_not_clear_errors_or_pending_prelogin() {
    let mut recovery = Recovery::default();
    recovery.failure(false);
    recovery.page_loaded();
    assert_eq!(recovery.status, RecoveryStatus::Rejected);
    recovery.retry();
    recovery.page_loaded();
    assert_eq!(recovery.status, RecoveryStatus::Restarting);
    recovery.status = RecoveryStatus::Loading;
    recovery.page_loaded();
    assert_eq!(recovery.status, RecoveryStatus::Hidden);
  }
}
