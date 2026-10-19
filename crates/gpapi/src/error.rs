use thiserror::Error;

mod server_tls;
pub use server_tls::{ServerTlsError, ServerTlsErrorKind};

#[derive(Error, Debug)]
pub enum PortalError {
  #[error("Prelogin error: {0}")]
  PreloginError(String),

  #[error("Portal config error: {0}")]
  ConfigError(String),

  #[error(transparent)]
  NetworkError(#[from] reqwest::Error),
}

impl PortalError {
  fn tls_error_kind(&self) -> Option<ServerTlsErrorKind> {
    match self {
      Self::NetworkError(error) => server_tls::classify(error),
      _ => None,
    }
  }
}

#[derive(Error, Debug)]
pub enum AuthDataParseError {
  #[error("No auth data found")]
  NotFound,
  #[error("Authentication failed (SAML status: {0})")]
  AuthenticationFailed(String),
  #[error(transparent)]
  Invalid(#[from] anyhow::Error),
}

impl AuthDataParseError {
  pub fn is_invalid(&self) -> bool {
    matches!(
      self,
      AuthDataParseError::Invalid(_) | AuthDataParseError::AuthenticationFailed(_)
    )
  }
}
