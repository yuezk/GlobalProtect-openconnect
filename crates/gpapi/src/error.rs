use thiserror::Error;

#[derive(Error, Debug)]
pub enum PortalError {
  #[error("Prelogin error: {0}")]
  PreloginError(String),

  #[error("Portal config error: {0}")]
  ConfigError(String),

  #[error(transparent)]
  NetworkError(#[from] reqwest::Error),

  #[error("TLS error")]
  TlsError,
}

impl PortalError {
  pub fn is_legacy_openssl_error(&self) -> bool {
    // Check for the specific error message in the underlying reqwest error
    match self {
      PortalError::NetworkError(e) => {
        let error_msg = e.to_string();
        error_msg.contains("unsafe legacy renegotiation")
      }
      _ => false,
    }
  }

  pub fn is_tls_error(&self) -> bool {
    match self {
      PortalError::TlsError => true,
      PortalError::NetworkError(e) => {
        let error_msg = e.to_string();
        error_msg.contains("certificate verify failed")
      }
      _ => false,
    }
  }
}

#[derive(Error, Debug)]
pub enum AuthDataParseError {
  #[error("No auth data found")]
  NotFound,
  #[error(transparent)]
  Invalid(#[from] anyhow::Error),
}

impl AuthDataParseError {
  pub fn is_invalid(&self) -> bool {
    matches!(self, AuthDataParseError::Invalid(_))
  }
}
