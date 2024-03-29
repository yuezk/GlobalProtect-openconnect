use thiserror::Error;

#[derive(Error, Debug)]
pub enum PortalError {
  #[error("Portal prelogin error: {0}")]
  PreloginError(String),
  #[error("Portal config error: {0}")]
  ConfigError(String),
  #[error("Network error: {0}")]
  NetworkError(String),
}
