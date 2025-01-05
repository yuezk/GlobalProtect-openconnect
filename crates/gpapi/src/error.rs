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
    format!("{:?}", self).contains("unsafe legacy renegotiation")
  }

  pub fn is_tls_error(&self) -> bool {
    matches!(self, PortalError::TlsError) || format!("{:?}", self).contains("certificate verify failed")
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
