/// A TLS failure that can be presented without exposing request parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TlsFailureReason {
  Expired,
  NotYetValid,
  Untrusted,
  HostnameMismatch,
  InvalidCertificate,
  Handshake,
}

impl TlsFailureReason {
  pub fn is_certificate_error(self) -> bool {
    self != Self::Handshake
  }

  pub fn message(self) -> &'static str {
    match self {
      Self::Expired => "The certificate has expired.",
      Self::NotYetValid => "The certificate is not valid yet.",
      Self::Untrusted => "The certificate was issued by an untrusted authority.",
      Self::HostnameMismatch => "The certificate does not match this server’s hostname.",
      Self::InvalidCertificate => "The server’s certificate could not be verified.",
      Self::Handshake => "The server could not establish a compatible TLS connection.",
    }
  }
}

#[derive(Clone, Debug)]
pub(super) struct TlsFailure {
  pub host: String,
  pub reason: TlsFailureReason,
  pub can_continue: bool,
}

impl TlsFailure {
  pub fn confirmation_title(&self) -> String {
    format!("Continue to {}?", self.host)
  }

  pub fn confirmation_message(&self) -> String {
    format!(
      "{} Your sign-in information could be intercepted.\n\nThis exception applies only to this host until this sign-in window closes.",
      self.reason.message()
    )
  }
}
